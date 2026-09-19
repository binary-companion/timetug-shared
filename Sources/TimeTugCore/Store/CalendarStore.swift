import Foundation

public struct CalendarSnapshot: Sendable {
    public let events: [CalendarEvent]
    public let calendars: [CalendarInfo]
    /// Keyed by `CalendarSource.id`.
    public let statuses: [String: SourceStatus]
    /// `CalendarSource.id` -> `displayName`, for front ends that show source problems.
    public let sourceNames: [String: String]
    public let fetchedAt: Date
    /// Look-alike events kept separate (event id -> other events), for a manual "Merge".
    public let candidates: [String: [CalendarEvent]]

    public init(events: [CalendarEvent], calendars: [CalendarInfo], statuses: [String: SourceStatus],
                sourceNames: [String: String], fetchedAt: Date, candidates: [String: [CalendarEvent]] = [:]) {
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

    private let sources: [any CalendarSource]
    private let calendar: Calendar
    private var lastEvents: [String: [CalendarEvent]] = [:]
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

    /// Local midnight today through next local midnight + lead time + buffer.
    public func fetchWindow(now: Date, leadTime: TimeInterval) -> DateInterval {
        let dayStart = calendar.startOfDay(for: now)
        let nextDayStart = calendar.date(byAdding: .day, value: 1, to: dayStart)!
        return DateInterval(start: dayStart, end: nextDayStart.addingTimeInterval(leadTime + Self.fetchBuffer))
    }

    public func refresh(now: Date, leadTime: TimeInterval) async -> CalendarSnapshot {
        let window = fetchWindow(now: now, leadTime: leadTime)

        let results = await withTaskGroup(
            of: (String, Result<([CalendarInfo], [CalendarEvent]), Error>).self
        ) { group in
            for source in sources {
                group.addTask {
                    do {
                        let calendars = try await source.calendars()
                        let events = try await source.events(in: window)
                        return (source.id, .success((calendars, events)))
                    } catch {
                        return (source.id, .failure(error))
                    }
                }
            }
            var collected: [(String, Result<([CalendarInfo], [CalendarEvent]), Error>)] = []
            for await result in group { collected.append(result) }
            return collected
        }

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
    public func unmerge(_ event: CalendarEvent, now: Date) -> CalendarSnapshot {
        let parts = event.participants
        for (index, a) in parts.enumerated() {
            for b in parts[(index + 1)...] { lessons.record(a, b, decision: .different, now: now) }
        }
        return makeSnapshot(now: now)
    }

    /// The user says one copy of a merged event is not the same meeting as the rest: remember a "different"
    /// lesson between it and every other participant, leaving the others as they were.
    public func separate(_ member: MergedMember, from event: CalendarEvent, now: Date) -> CalendarSnapshot {
        for other in event.participants where other.contentKey != member.contentKey || other.calendarKey != member.calendarKey {
            lessons.record(member, other, decision: .different, now: now)
        }
        return makeSnapshot(now: now)
    }

    /// The user says these two displayed events are one meeting.
    public func merge(_ a: CalendarEvent, _ b: CalendarEvent, now: Date) -> CalendarSnapshot {
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
        let calendars = sources.flatMap { lastCalendars[$0.id] ?? [] }
        let raw = sources.flatMap { source in
            (lastEvents[source.id] ?? []).filter { $0.end > window.start && $0.start < window.end }
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
            events: resolution.events, calendars: calendars, statuses: statuses,
            sourceNames: Dictionary(uniqueKeysWithValues: sources.map { ($0.id, $0.displayName) }),
            fetchedAt: now, candidates: resolution.candidates)
    }
}
