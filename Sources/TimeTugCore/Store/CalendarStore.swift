import CalendarCore
import Foundation

public struct CalendarSnapshot: Sendable {
    public let events: [TimeTugCalendarEvent]
    /// Deduplicated events spanning today and the following two local days, for widgets only.
    public let widgetEvents: [TimeTugCalendarEvent]
    public let calendars: [CalendarInfo]
    /// Keyed by `CalendarSource.id`.
    public let statuses: [String: SourceStatus]
    /// `CalendarSource.id` -> `displayName`, for front ends that show source problems.
    public let sourceNames: [String: String]
    public let fetchedAt: Date
    /// Increases only when the store accepts a new publication, including a local settings/lesson change.
    public let revision: UInt64
    /// Quiet hints per source (a warning icon), keyed by `CalendarSource.id`; a source with none has no entry.
    public let notices: [String: [SourceNotice]]
    /// Look-alike events kept separate (event id -> other events), for a manual "Merge".
    public let candidates: [String: [TimeTugCalendarEvent]]

    public init(events: [TimeTugCalendarEvent], calendars: [CalendarInfo], statuses: [String: SourceStatus],
                sourceNames: [String: String], fetchedAt: Date, candidates: [String: [TimeTugCalendarEvent]] = [:],
                revision: UInt64 = 0, widgetEvents: [TimeTugCalendarEvent] = [],
                notices: [String: [SourceNotice]] = [:]) {
        self.notices = notices
        self.events = events
        self.widgetEvents = widgetEvents
        self.calendars = calendars
        self.statuses = statuses
        self.sourceNames = sourceNames
        self.fetchedAt = fetchedAt
        self.revision = revision
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
    private var refreshRevision: UInt64 = 0
    private var sourceRequestRevisions: [String: UInt64] = [:]
    private var requestedQueryWindow: DateInterval?
    private var publicationRevision: UInt64 = 0
    private var latestSnapshot = CalendarSnapshot.empty
    private var calendar: Calendar
    private var lastEvents: [String: [TimeTugCalendarEvent]] = [:]
    private var lastCalendars: [String: [CalendarInfo]] = [:]
    private var statuses: [String: SourceStatus] = [:]
    private var lastNotices: [String: [SourceNotice]] = [:]

    private static let maxPendingPerPass = 20
    private let adjudicator: (any DuplicateAdjudicator)?
    private var inferenceEnabled = false
    private var lessons = LessonBook()
    private var verdicts = VerdictCache()
    private var pending: [AdjudicationRequest] = []
    private var inferenceGeneration: UInt64 = 0
    private var failures: [String: (attempts: Int, retryAfter: Date)] = [:]
    private let passClock: @Sendable () -> Date
    private var lastWindow: DateInterval?
    private var lastWidgetWindow: DateInterval?
    private var isResolving = false

    public init(sources: [any CalendarSource], calendar: Calendar = .current,
                adjudicator: (any DuplicateAdjudicator)? = nil,
                passClock: @escaping @Sendable () -> Date = Date.init) {
        self.sources = sources
        self.calendar = calendar
        self.adjudicator = adjudicator
        self.passClock = passClock
    }

    /// Replaces the source set. Removed sources lose their cached events, calendars and status. Bumps the
    /// generation so a `refresh` that was suspended when this ran discards its results instead of restoring them.
    public func setSources(_ newSources: [any CalendarSource]) {
        let keep = Set(newSources.map(\.id))
        lastEvents = lastEvents.filter { keep.contains($0.key) }
        lastCalendars = lastCalendars.filter { keep.contains($0.key) }
        statuses = statuses.filter { keep.contains($0.key) }
        sourceRequestRevisions = sourceRequestRevisions.filter { keep.contains($0.key) }
        lastNotices = lastNotices.filter { keep.contains($0.key) }
        sources = newSources
        generation += 1
        invalidateInference()
        refreshRevision &+= 1
        if lastWindow != nil { _ = makeSnapshot(now: latestSnapshot.fetchedAt) }
    }

    /// Changes the calendar context used for day windows and all-day filtering.
    /// Any refresh started in the previous context must discard its results.
    public func setCalendar(_ newCalendar: Calendar) {
        calendar = newCalendar
        refreshRevision &+= 1
        invalidateInference()
    }

    /// Local midnight today through next local midnight + lead time + buffer.
    public func fetchWindow(now: Date, leadTime: TimeInterval) -> DateInterval {
        let dayStart = calendar.startOfDay(for: now)
        let nextDayStart = calendar.date(byAdding: .day, value: 1, to: dayStart)!
        return DateInterval(start: dayStart, end: nextDayStart.addingTimeInterval(leadTime + Self.fetchBuffer))
    }

    /// Refreshes only named sources when the fetch window is unchanged. A new day/window forces a full read.
    public func refresh(now: Date, leadTime: TimeInterval, sourceIDs: Set<String>? = nil,
                        onPublication: (@Sendable (CalendarSnapshot) async -> Void)? = nil) async -> CalendarSnapshot {
        let window = fetchWindow(now: now, leadTime: leadTime)
        let widgetEnd = calendar.date(byAdding: .day, value: WidgetSnapshot.horizonDays,
                                      to: window.start)!
        let widgetWindow = DateInterval(start: window.start, end: widgetEnd)
        let queryWindow = DateInterval(
            start: window.start.addingTimeInterval(-Self.sourceQueryMargin),
            end: max(window.end, widgetWindow.end).addingTimeInterval(Self.sourceQueryMargin))
        let startedGeneration = generation
        let contextChanged = requestedQueryWindow != queryWindow
        if contextChanged {
            refreshRevision &+= 1
            requestedQueryWindow = queryWindow
        }
        let startedRefresh = refreshRevision
        let current = sources.filter { contextChanged || sourceIDs == nil || sourceIDs!.contains($0.id) }
        lastWindow = window
        lastWidgetWindow = widgetWindow
        guard !current.isEmpty else {
            let snapshot = makeSnapshot(now: now)
            if let onPublication { await onPublication(snapshot) }
            return snapshot
        }

        return await withTaskGroup(
            of: (String, UInt64, Result<([CalendarInfo], [TimeTugCalendarEvent], [SourceNotice]), Error>).self
        ) { group in
            for source in current {
                let revision = (sourceRequestRevisions[source.id] ?? 0) &+ 1
                sourceRequestRevisions[source.id] = revision
                group.addTask {
                    do {
                        let calendars = try await source.calendars()
                        let events = try await source.events(in: queryWindow)
                        return (source.id, revision, .success((calendars, events, await source.notices())))
                    } catch {
                        return (source.id, revision, .failure(error))
                    }
                }
            }
            for await (sourceID, revision, result) in group {
                guard generation == startedGeneration, refreshRevision == startedRefresh,
                      sourceRequestRevisions[sourceID] == revision else { continue }
                switch result {
                case .success(let (calendars, events, notices)):
                    lastCalendars[sourceID] = calendars
                    lastEvents[sourceID] = events
                    lastNotices[sourceID] = notices
                    statuses[sourceID] = .ok
                case .failure(let error):
                    lastNotices[sourceID] = nil
                    switch error {
                    case SourceError.needsPermission: statuses[sourceID] = .needsPermission
                    case SourceError.authExpired: statuses[sourceID] = .authExpired
                    default: statuses[sourceID] = .failing(String(describing: error))
                    }
                }
                let snapshot = makeSnapshot(now: now)
                if let onPublication { await onPublication(snapshot) }
            }
            return latestSnapshot
        }
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
        if inferenceEnabled != enabled { invalidateInference() }
        inferenceEnabled = enabled
        return makeSnapshot(now: now)
    }

    private func invalidateInference() {
        inferenceGeneration &+= 1
        failures.removeAll()
    }

    /// Asks the engine about the pairs still waiting for a verdict (one bounded pass). Returns a new
    /// snapshot only when a verdict was recorded; call again until nil to drain a backlog.
    public func resolvePending(now: Date) async -> CalendarSnapshot? {
        guard let adjudicator, let engine = activeEngine, !pending.isEmpty, !isResolving else { return nil }
        isResolving = true
        defer { isResolving = false }
        let batch = Array(pending.filter { now >= (failures[$0.id]?.retryAfter ?? .distantPast) }
            .prefix(Self.maxPendingPerPass))
        guard !batch.isEmpty else { return nil }
        let startedGeneration = inferenceGeneration
        let startedAt = passClock()
        guard passClock().timeIntervalSince(startedAt) < 30 else { return nil }
        let returned = await adjudicator.judge(batch)
        guard inferenceGeneration == startedGeneration, !Task.isCancelled,
              activeEngine?.id == engine.id else { return nil }
        let byID = Dictionary(batch.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        let returnedIDs = Set(returned.map(\.requestID))
        for request in batch where !returnedIDs.contains(request.id) {
            let attempts = min((failures[request.id]?.attempts ?? 0) + 1, 5)
            let delay = min(60 * (1 << (attempts - 1)), 900)
            failures[request.id] = (attempts, now.addingTimeInterval(TimeInterval(delay)))
        }
        var changed = false
        for verdict in returned {
            guard let request = byID[verdict.requestID] else { continue }
            failures[verdict.requestID] = nil
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
        invalidateInference()
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
        invalidateInference()
        let chosen = Set(members.map { "\($0.calendarKey)|\($0.contentKey)" })
        let rest = event.participants.filter { !chosen.contains("\($0.calendarKey)|\($0.contentKey)") }
        for member in members {
            for other in rest { lessons.record(member, other, decision: .different, now: now) }
        }
        return makeSnapshot(now: now)
    }

    /// The user says these two displayed events are one meeting.
    public func merge(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent, now: Date) -> CalendarSnapshot {
        invalidateInference()
        for x in a.participants { for y in b.participants { lessons.record(x, y, decision: .same, now: now) } }
        return makeSnapshot(now: now)
    }

    /// Also clears cached model verdicts, so a pair the model merged is asked about again.
    public func forgetLessons(now: Date) -> CalendarSnapshot {
        invalidateInference()
        lessons = LessonBook()
        verdicts = VerdictCache()
        return makeSnapshot(now: now)
    }

    public func state() -> DedupState { DedupState(lessons: lessons, verdicts: verdicts) }

    public func load(_ state: DedupState) {
        invalidateInference()
        lessons = state.lessons
        verdicts = state.verdicts
        verdicts.discardIncompatible()
    }

    private func makeSnapshot(now: Date) -> CalendarSnapshot {
        publicationRevision &+= 1
        let window = lastWindow ?? DateInterval(start: now, duration: 0)
        let ids = Set(sources.map(\.id))
        let calendars = sources.flatMap { lastCalendars[$0.id] ?? [] }
        let widgetWindow = lastWidgetWindow ?? window
        let firstDate = AllDay.date(of: widgetWindow.start, in: calendar.timeZone)
        let lastDate = AllDay.date(of: widgetWindow.end.addingTimeInterval(-1), in: calendar.timeZone)
        var raw = sources.flatMap { source in
            (lastEvents[source.id] ?? []).filter { event in
                if let dates = event.allDayDates { return dates.endExclusive > firstDate && dates.first <= lastDate }
                return event.end > widgetWindow.start && event.start < widgetWindow.end
            }
        }
        let infoByKey = Dictionary(calendars.map { ($0.key, $0) }, uniquingKeysWith: { first, _ in first })
        for index in raw.indices {
            let info = infoByKey[raw[index].calendarKey]
            raw[index].calendarService = info?.service?.rawValue
            raw[index].calendarProvider = info?.provider?.rawValue
        }
        let currentFirstDate = AllDay.date(of: window.start, in: calendar.timeZone)
        let currentLastDate = AllDay.date(of: window.end.addingTimeInterval(-1), in: calendar.timeZone)
        let current = raw.filter { event in
            if let dates = event.allDayDates { return dates.endExclusive > currentFirstDate && dates.first <= currentLastDate }
            return event.end > window.start && event.start < window.end
        }
        let resolution = DuplicateResolver.resolve(
            events: current, calendars: calendars, lessons: lessons, verdicts: activeEngine == nil ? nil : verdicts,
            engineID: activeEngine?.id ?? "unknown")
        let widgetResolution = DuplicateResolver.resolve(
            events: raw, calendars: calendars, lessons: lessons, verdicts: activeEngine == nil ? nil : verdicts,
            engineID: activeEngine?.id ?? "unknown")
        lessons.touch(resolution.usedLessonKeys, now: now)
        lessons.prune(now: now)
        _ = verdicts.prune(now: now)
        if pending.map(\.id) != resolution.pending.map(\.id) {
            inferenceGeneration &+= 1
            failures = failures.filter { key, _ in resolution.pending.contains { $0.id == key } }
        }
        pending = resolution.pending
        let snapshot = CalendarSnapshot(
            events: resolution.events, calendars: calendars, statuses: statuses.filter { ids.contains($0.key) },
            sourceNames: Dictionary(uniqueKeysWithValues: sources.map { ($0.id, $0.displayName) }),
            fetchedAt: now, candidates: resolution.candidates, revision: publicationRevision,
            widgetEvents: widgetResolution.events,
            notices: lastNotices.filter { ids.contains($0.key) && !$0.value.isEmpty })
        latestSnapshot = snapshot
        return snapshot
    }
}
