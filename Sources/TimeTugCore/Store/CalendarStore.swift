import CalendarCore
import Foundation

public struct CalendarSnapshot: Sendable {
    public let events: [TimeTugCalendarEvent]
    public let calendars: [CalendarInfo]
    /// Keyed by `CalendarSource.id`.
    public let statuses: [String: SourceStatus]
    /// `CalendarSource.id` -> `displayName`, for front ends that show source problems.
    public let sourceNames: [String: String]
    public let fetchedAt: Date
    /// Look-alike events kept separate (event id -> other events), for a manual "Merge".
    public let candidates: [String: [TimeTugCalendarEvent]]

    public init(events: [TimeTugCalendarEvent], calendars: [CalendarInfo], statuses: [String: SourceStatus],
                sourceNames: [String: String], fetchedAt: Date, candidates: [String: [TimeTugCalendarEvent]] = [:]) {
        self.events = events
        self.calendars = calendars
        self.statuses = statuses
        self.sourceNames = sourceNames
        self.fetchedAt = fetchedAt
        self.candidates = candidates
    }

    public static let empty = CalendarSnapshot(
        events: [], calendars: [], statuses: [:], sourceNames: [:], fetchedAt: .distantPast)
}

public enum InferenceStatus: Equatable, Sendable {
    case disabled
    case noEngine
    case unavailable(reason: String)
    case notOnDevice
    case active(EngineInfo)
}

/// What the app persists between launches so decisions and verdicts survive a relaunch.
public struct DedupState: Codable, Equatable, Sendable {
    public var lessons: LessonBook
    public var verdicts: VerdictCache
    public init(lessons: LessonBook = LessonBook(), verdicts: VerdictCache = VerdictCache()) {
        self.lessons = lessons
        self.verdicts = verdicts
    }
}

/// Merges events from all sources over the fetch window. A failing source keeps its last good
/// events so a network blip never causes a missed meeting.
public actor CalendarStore {
    /// Extra time past the lead time so events just after midnight are already loaded.
    public static let fetchBuffer: TimeInterval = 300
    /// The widest gap between two zones, so an all-day event on one of today's dates in a far zone is still fetched.
    public static let sourceQueryMargin: TimeInterval = 26 * 3600

    private var sources: [any CalendarSource]
    private var generation = 0
    private let calendar: Calendar
    private var lastEvents: [String: [TimeTugCalendarEvent]] = [:]
    private var lastCalendars: [String: [CalendarInfo]] = [:]
    private var statuses: [String: SourceStatus] = [:]

    private static let maxPendingPerPass = 20
    private let adjudicator: (any DuplicateAdjudicator)?
    private var inferenceEnabled = false
    private var lessons = LessonBook()
    private var verdicts = VerdictCache()
    private var pending: [AdjudicationRequest] = []
    private var lastWindow: DateInterval?
    private var isResolving = false

    public init(sources: [any CalendarSource], calendar: Calendar = .current,
                adjudicator: (any DuplicateAdjudicator)? = nil) {
        self.sources = sources
        self.calendar = calendar
        self.adjudicator = adjudicator
    }

    /// Replaces the source set. Removed sources lose their cached events, calendars and status. Bumps the
    /// generation so a `refresh` that was suspended when this ran discards its results instead of restoring them.
    public func setSources(_ newSources: [any CalendarSource]) {
        let keep = Set(newSources.map(\.id))
        lastEvents = lastEvents.filter { keep.contains($0.key) }
        lastCalendars = lastCalendars.filter { keep.contains($0.key) }
        statuses = statuses.filter { keep.contains($0.key) }
        sources = newSources
        generation += 1
    }

    /// Local midnight today through next local midnight + lead time + buffer.
    public func fetchWindow(now: Date, leadTime: TimeInterval) -> DateInterval {
        let dayStart = calendar.startOfDay(for: now)
        let nextDayStart = calendar.date(byAdding: .day, value: 1, to: dayStart)!
        return DateInterval(start: dayStart, end: nextDayStart.addingTimeInterval(leadTime + Self.fetchBuffer))
    }

    public func refresh(now: Date, leadTime: TimeInterval) async -> CalendarSnapshot {
        let window = fetchWindow(now: now, leadTime: leadTime)
        let queryWindow = DateInterval(
            start: window.start.addingTimeInterval(-Self.sourceQueryMargin),
            end: window.end.addingTimeInterval(Self.sourceQueryMargin))
        let startedGeneration = generation
        let current = sources

        let results = await withTaskGroup(
            of: (String, Result<([CalendarInfo], [TimeTugCalendarEvent]), Error>).self
        ) { group in
            for source in current {
                group.addTask {
                    do {
                        let calendars = try await source.calendars()
                        let events = try await source.events(in: queryWindow)
                        return (source.id, .success((calendars, events)))
                    } catch {
                        return (source.id, .failure(error))
                    }
                }
            }
            var collected: [(String, Result<([CalendarInfo], [TimeTugCalendarEvent]), Error>)] = []
            for await result in group { collected.append(result) }
            return collected
        }

        if generation == startedGeneration {
            for (sourceID, result) in results {
                switch result {
                case .success(let (calendars, events)):
                    lastCalendars[sourceID] = calendars
                    lastEvents[sourceID] = events
                    statuses[sourceID] = .ok
                case .failure(let error):
                    switch error {
                    case SourceError.needsPermission: statuses[sourceID] = .needsPermission
                    case SourceError.authExpired: statuses[sourceID] = .authExpired
                    default: statuses[sourceID] = .failing(String(describing: error))
                    }
                }
            }
        }

        lastWindow = window
        return makeSnapshot(now: now)
    }

    private var activeEngine: EngineInfo? {
        guard inferenceEnabled, let adjudicator, case .available(let engine) = adjudicator.availability,
              engine.isOnDevice else { return nil }
        return engine
    }

    public func inferenceStatus() -> InferenceStatus {
        guard inferenceEnabled else { return .disabled }
        guard let adjudicator else { return .noEngine }
        switch adjudicator.availability {
        case .unavailable(let reason): return .unavailable(reason: reason)
        case .available(let engine): return engine.isOnDevice ? .active(engine) : .notOnDevice
        }
    }

    public func setInferenceEnabled(_ enabled: Bool, now: Date) -> CalendarSnapshot {
        inferenceEnabled = enabled
        return makeSnapshot(now: now)
    }

    /// Asks the engine about the pairs still waiting for a verdict (one bounded pass). Returns a new
    /// snapshot only when a verdict was recorded; call again until nil to drain a backlog.
    public func resolvePending(now: Date) async -> CalendarSnapshot? {
        guard let adjudicator, let engine = activeEngine, !pending.isEmpty, !isResolving else { return nil }
        isResolving = true
        defer { isResolving = false }
        let batch = Array(pending.prefix(Self.maxPendingPerPass))
        let returned = await adjudicator.judge(batch)
        let byID = Dictionary(batch.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        var changed = false
        for verdict in returned {
            guard let request = byID[verdict.requestID] else { continue }
            verdicts.store(verdict, engine: engine, end: max(request.first.end, request.second.end), now: now)
            changed = true
        }
        guard changed else { return nil }
        _ = verdicts.prune(now: now)
        return makeSnapshot(now: now)
    }

    /// The user says this merged event is not one meeting: remember every pair of its participants.
    /// `LessonBook.record` skips same-calendar pairs unless they are exact duplicates.
    public func unmerge(_ event: TimeTugCalendarEvent, now: Date) -> CalendarSnapshot {
        let parts = event.participants
        for (index, a) in parts.enumerated() {
            for b in parts[(index + 1)...] { lessons.record(a, b, decision: .different, now: now) }
        }
        return makeSnapshot(now: now)
    }

    /// The user says these copies of a merged event are not the same meeting as the rest: remember a "different"
    /// lesson between each given copy and every other participant not in `members`. Pairs among the given
    /// copies are not recorded, so they stay together.
    public func separate(_ members: [MergedMember], from event: TimeTugCalendarEvent, now: Date) -> CalendarSnapshot {
        let chosen = Set(members.map { "\($0.calendarKey)|\($0.contentKey)" })
        let rest = event.participants.filter { !chosen.contains("\($0.calendarKey)|\($0.contentKey)") }
        for member in members {
            for other in rest { lessons.record(member, other, decision: .different, now: now) }
        }
        return makeSnapshot(now: now)
    }

    /// The user says these two displayed events are one meeting.
    public func merge(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent, now: Date) -> CalendarSnapshot {
        for x in a.participants { for y in b.participants { lessons.record(x, y, decision: .same, now: now) } }
        return makeSnapshot(now: now)
    }

    /// Also clears cached model verdicts, so a pair the model merged is asked about again.
    public func forgetLessons(now: Date) -> CalendarSnapshot {
        lessons = LessonBook()
        verdicts = VerdictCache()
        return makeSnapshot(now: now)
    }

    public func state() -> DedupState { DedupState(lessons: lessons, verdicts: verdicts) }

    public func load(_ state: DedupState) {
        lessons = state.lessons
        verdicts = state.verdicts
    }

    private func makeSnapshot(now: Date) -> CalendarSnapshot {
        let window = lastWindow ?? DateInterval(start: now, duration: 0)
        let ids = Set(sources.map(\.id))
        let calendars = sources.flatMap { lastCalendars[$0.id] ?? [] }
        let firstDate = AllDay.date(of: window.start, in: calendar.timeZone)
        let lastDate = AllDay.date(of: window.end.addingTimeInterval(-1), in: calendar.timeZone)
        let raw = sources.flatMap { source in
            (lastEvents[source.id] ?? []).filter { event in
                if let dates = event.allDayDates { return dates.endExclusive > firstDate && dates.first <= lastDate }
                return event.end > window.start && event.start < window.end
            }
        }
        var resolution = DuplicateResolver.resolve(
            events: raw, calendars: calendars, lessons: lessons, verdicts: activeEngine == nil ? nil : verdicts)
        lessons.touch(resolution.usedLessonKeys, now: now)
        lessons.prune(now: now)
        _ = verdicts.prune(now: now)
        pending = resolution.pending
        for index in resolution.events.indices where resolution.events[index].conferenceURL == nil {
            resolution.events[index].conferenceURL = ConferenceLinkDetector.detect(
                location: resolution.events[index].location, url: resolution.events[index].url,
                notes: resolution.events[index].notes)
        }
        return CalendarSnapshot(
            events: resolution.events, calendars: calendars, statuses: statuses.filter { ids.contains($0.key) },
            sourceNames: Dictionary(uniqueKeysWithValues: sources.map { ($0.id, $0.displayName) }),
            fetchedAt: now, candidates: resolution.candidates)
    }
}
