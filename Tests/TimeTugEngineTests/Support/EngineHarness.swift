import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Suspends callers of `wait()` until released one by one (by arrival index) or all at once.
actor CallGate {
    private var waiters: [CheckedContinuation<Void, Never>] = []
    var waiting: Int { waiters.count }
    func wait() async { await withCheckedContinuation { waiters.append($0) } }
    /// Resumes the caller that is `index`-th among those still waiting (0 = earliest).
    func release(_ index: Int) { waiters.remove(at: index).resume() }
    func releaseAll() {
        let all = waiters
        waiters = []
        all.forEach { $0.resume() }
    }
}

/// A connector-library source with scripted events: it can fail, hold each `events(in:)` call at a gate, count
/// calls, and emit change signals on demand. A gated call returns the events and failure scripted when it began.
final class ScriptedSource: CalendarCore.CalendarSource, @unchecked Sendable {
    struct Script: Sendable {
        var events: [CalendarCore.CalendarEvent]
        var failure: CalendarCore.SourceError?
        var gated = false
        var eventsCalls = 0
    }

    let id: String
    var displayName: String { id }
    var capabilities: SourceCapabilities { SourceCapabilities() }
    let gate = CallGate()
    private let script: LockedBox<Script>
    private let stream: AsyncStream<CalendarChange>
    private let continuation: AsyncStream<CalendarChange>.Continuation

    init(id: String, events: [CalendarCore.CalendarEvent], failure: CalendarCore.SourceError? = nil) {
        self.id = id
        script = LockedBox(Script(events: events, failure: failure))
        (stream, continuation) = AsyncStream.makeStream(of: CalendarChange.self)
    }

    var eventsCalls: Int { script.get().eventsCalls }
    func setEvents(_ events: [CalendarCore.CalendarEvent]) { script.mutate { $0.events = events } }
    func setFailure(_ failure: CalendarCore.SourceError?) { script.mutate { $0.failure = failure } }
    func setGated(_ gated: Bool) { script.mutate { $0.gated = gated } }
    func signal(_ change: CalendarChange) { continuation.yield(change) }

    func calendars() async throws -> [CalendarDescriptor] {
        [CalendarDescriptor(id: "cal", title: "Work", service: .google)]
    }

    func events(in interval: DateInterval) async throws -> [CalendarCore.CalendarEvent] {
        let current = script.mutate { script -> Script in
            script.eventsCalls += 1
            return script
        }
        if current.gated { await gate.wait() }
        if let failure = current.failure { throw failure }
        return current.events
    }

    func changes() -> AsyncStream<CalendarChange> { stream }
}

extension CalendarCore.CalendarEvent {
    /// A 30-minute meeting on calendar "cal" starting `startsIn` seconds after `EngineHarness.epoch`.
    static func sample(id: String, startsIn: TimeInterval, duration: TimeInterval = 1800) -> CalendarCore.CalendarEvent {
        let start = EngineHarness.epoch.addingTimeInterval(startsIn)
        return CalendarCore.CalendarEvent(
            eventID: id, calendarID: "cal", title: "Meeting \(id)", start: start, end: start.addingTimeInterval(duration))
    }
}

/// A real `Engine` over a temp data directory, a `ManualClock` at `epoch`, in-memory settings, captured
/// diagnostics and events, and a fake connector kind "fake" with one scripted source per stored account.
struct EngineHarness {
    /// 2026-10-15 12:00:00 UTC.
    static let epoch = Date(timeIntervalSince1970: 1_792_065_600)
    static let utc: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        return calendar
    }()

    let clock: ManualClock
    let directory: URL
    let settings: InMemorySettingsStorage
    let diagnostics: LockedBox<[EngineDiagnostic]>
    let emitted: LockedBox<[EngineEvent]>
    /// Keyed by connection id.
    let sources: [String: ScriptedSource]
    let kind: FakeConnectorKind
    let engine: Engine

    /// `events` go to account "c1"; `accounts` maps further connection ids to their events. With `directory`,
    /// an existing data directory is reused and `accounts.json` is left as it is.
    init(
        events: [CalendarCore.CalendarEvent] = [], accounts: [String: [CalendarCore.CalendarEvent]] = [:],
        failures: [String: CalendarCore.SourceError] = [:], directory: URL? = nil
    ) async throws {
        var all = accounts
        if all["c1"] == nil { all["c1"] = events }
        let dir = try directory ?? TempDir.make()
        self.directory = dir
        if directory == nil {
            let store = FileConnectionStore(url: dir.appendingPathComponent("accounts.json"))
            for id in all.keys.sorted() {
                try await store.add(Connection(kindID: "fake", connectionID: id, displayName: "\(id)@x.test"))
            }
        }
        var built: [String: ScriptedSource] = [:]
        for (id, list) in all {
            built[id] = ScriptedSource(id: "fake-\(id)", events: list, failure: failures[id])
        }
        sources = built
        let sources = built
        clock = ManualClock(start: Self.epoch)
        var initial = EngineSettings(systemCalendarEnabled: false)
        initial.takeover.takeoverCalendarKeys = Set(all.keys.map { "fake-\($0)/cal" })
        settings = InMemorySettingsStorage(initial)
        diagnostics = LockedBox([])
        emitted = LockedBox([])
        kind = FakeConnectorKind(id: "fake", displayName: "Fake")
        kind.sourceFactory = { connection in
            guard let source = sources[connection.connectionID] else {
                throw CalendarCore.SourceError.invalidResponse("no scripted source")
            }
            return source
        }
        var registry = ConnectorRegistry()
        registry.register(kind)
        let signIn = FakeSignIn()
        let diagnostics = diagnostics
        let host = EngineHost(
            dataDirectory: dir, settings: settings, credentials: InMemoryCredentialStore(), interaction: signIn,
            signIn: signIn, registry: registry, diagnostics: { event in diagnostics.mutate { $0.append(event) } },
            clock: clock, calendar: { Self.utc })
        engine = Engine(host: host)
        let stream = await engine.events
        let emitted = emitted
        Task { for await event in stream { emitted.mutate { $0.append(event) } } }
    }

    func source(_ connectionID: String = "c1") -> ScriptedSource { sources[connectionID]! }

    /// Polls `currentState()` (bounded, with `Task.yield()`) until `predicate` holds; records an issue on timeout.
    @discardableResult
    func waitForState(
        _ predicate: (EngineState) -> Bool, sourceLocation: SourceLocation = #_sourceLocation
    ) async -> EngineState {
        for _ in 0..<50_000 {
            let state = await engine.currentState()
            if predicate(state) { return state }
            await Task.yield()
        }
        let state = await engine.currentState()
        Issue.record("timed out waiting for state (revision \(state.revision))", sourceLocation: sourceLocation)
        return state
    }

    /// Polls (bounded, with `Task.yield()`) until `condition` holds; records an issue on timeout.
    func waitUntil(
        _ what: String, sourceLocation: SourceLocation = #_sourceLocation, _ condition: () async -> Bool
    ) async {
        for _ in 0..<50_000 {
            if await condition() { return }
            await Task.yield()
        }
        Issue.record("timed out waiting for \(what)", sourceLocation: sourceLocation)
    }

    /// Lets queued work (signal delivery through the listener chain) run; bounded, no wall-clock time.
    func settle() async {
        for _ in 0..<2_000 { await Task.yield() }
    }

    var takeoverDueIDs: [String] {
        emitted.get().compactMap { if case .takeoverDue(let request, _) = $0 { request.event.sourceEventID } else { nil } }
    }
}
