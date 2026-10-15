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
        var lastInterval: DateInterval?
    }

    let id: String
    var displayName: String { id }
    var capabilities: SourceCapabilities { SourceCapabilities() }
    let gate = CallGate()
    private let script: LockedBox<Script>
    private let stream: AsyncStream<CalendarChange>
    private let continuation: AsyncStream<CalendarChange>.Continuation
    private let terminated = LockedBox(false)
    private let sent = LockedBox(0)
    /// Change signals sent through `signal(_:)`.
    var signalsSent: Int { sent.get() }
    /// True once the change listener stopped consuming (cancelled or dropped).
    var listenerEnded: Bool { terminated.get() }

    init(id: String, events: [CalendarCore.CalendarEvent], failure: CalendarCore.SourceError? = nil) {
        self.id = id
        script = LockedBox(Script(events: events, failure: failure))
        (stream, continuation) = AsyncStream.makeStream(of: CalendarChange.self)
        let terminated = terminated
        continuation.onTermination = { _ in terminated.set(true) }
    }

    var eventsCalls: Int { script.get().eventsCalls }
    /// The interval the last `events(in:)` call asked for.
    var lastInterval: DateInterval? { script.get().lastInterval }
    func setEvents(_ events: [CalendarCore.CalendarEvent]) { script.mutate { $0.events = events } }
    func setFailure(_ failure: CalendarCore.SourceError?) { script.mutate { $0.failure = failure } }
    func setGated(_ gated: Bool) { script.mutate { $0.gated = gated } }
    func signal(_ change: CalendarChange) {
        sent.mutate { $0 += 1 }
        continuation.yield(change)
    }

    func calendars() async throws -> [CalendarDescriptor] {
        [CalendarDescriptor(id: "cal", title: "Work", service: .google)]
    }

    func events(in interval: DateInterval) async throws -> [CalendarCore.CalendarEvent] {
        let current = script.mutate { script -> Script in
            script.eventsCalls += 1
            script.lastInterval = interval
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
    /// What `host.calendar()` returns; a test changes it to simulate a time zone change.
    let hostCalendar: LockedBox<Calendar>
    /// How many times the engine asked the host for system calendar access.
    let systemAccessRequests: LockedBox<Int>
    let engine: Engine

    /// `events` go to account "c1"; `accounts` maps further connection ids to their events. With `directory`,
    /// an existing data directory is reused and `accounts.json` is left as it is. `sourceIDs` overrides a
    /// connection's source id (default "fake-<connection id>"); `clockStart` is where the clock starts;
    /// `hostSettings` replaces the in-memory settings the host hands the engine (`settings` is then unused).
    init(
        events: [CalendarCore.CalendarEvent] = [], accounts: [String: [CalendarCore.CalendarEvent]] = [:],
        failures: [String: CalendarCore.SourceError] = [:], directory: URL? = nil,
        textGenerator: (any TextGenerator)? = nil, inferenceEnabled: Bool = false,
        systemSource: (any TimeTugCore.CalendarSource)? = nil, sourceIDs: [String: String] = [:],
        clockStart: Date = EngineHarness.epoch, hostSettings: (any SettingsStorage)? = nil,
        systemAccessGate: Gate? = nil, extraKinds: [FakeConnectorKind] = [], unconfiguredKindIDs: [String] = []
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
            built[id] = ScriptedSource(id: sourceIDs[id] ?? "fake-\(id)", events: list, failure: failures[id])
        }
        sources = built
        let sources = built
        clock = ManualClock(start: clockStart)
        var initial = EngineSettings(systemCalendarEnabled: false)
        initial.takeover.takeoverCalendarKeys = Set(all.keys.map { "\(sourceIDs[$0] ?? "fake-\($0)")/cal" })
        initial.inferenceEnabled = inferenceEnabled
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
        for extra in extraKinds { registry.register(extra) }
        let signIn = FakeSignIn()
        let diagnostics = diagnostics
        let hostCalendar = LockedBox(Self.utc)
        self.hostCalendar = hostCalendar
        let systemAccessRequests = LockedBox(0)
        self.systemAccessRequests = systemAccessRequests
        let host = EngineHost(
            dataDirectory: dir, settings: hostSettings ?? settings, credentials: InMemoryCredentialStore(), interaction: signIn,
            signIn: signIn, registry: registry,
            unconfiguredKindIDs: unconfiguredKindIDs,
            buildSystemSource: systemSource.map { source in { @Sendable in source } },
            requestSystemAccess: systemSource == nil ? nil : { @Sendable in
                systemAccessRequests.mutate { $0 += 1 }
                await systemAccessGate?.wait()
            },
            textGenerator: textGenerator,
            diagnostics: { event in diagnostics.mutate { $0.append(event) } },
            clock: clock, calendar: { hostCalendar.get() })
        engine = Engine(host: host)
        let stream = await engine.events
        let emitted = emitted
        Task { for await event in stream { emitted.mutate { $0.append(event) } } }
    }

    func source(_ connectionID: String = "c1") -> ScriptedSource { sources[connectionID]! }

    /// Polls `currentState()` until `predicate` holds; fails loudly (with the state) after a generous bound.
    @discardableResult
    func waitForState(
        _ predicate: (EngineState) -> Bool, sourceLocation: SourceLocation = #_sourceLocation
    ) async -> EngineState {
        var last = await engine.currentState()
        await poll("state", dump: { await self.dump() }, sourceLocation: sourceLocation) {
            last = await engine.currentState()
            return predicate(last)
        }
        return last
    }

    /// Polls until `condition` holds; fails loudly (with the engine's state) after a generous bound.
    func waitUntil(
        _ what: String, sourceLocation: SourceLocation = #_sourceLocation, _ condition: () async -> Bool
    ) async {
        await poll(what, dump: { await self.dump() }, sourceLocation: sourceLocation, condition)
    }

    /// Extra drain conditions (for example a `TimelineRecorder` that must have received every event).
    let drainChecks = LockedBox<[@Sendable () -> Bool]>([])

    /// True when nothing is in flight: no command, refresh, resolution, account message or coordinator pass;
    /// every clock sleep that returned has been handled by the engine; every signal a source sent reached the
    /// coordinator; and every emitted event reached this harness (and each registered recorder).
    var isQuiescent: Bool {
        let activity = engine.activity
        return activity.isIdle
            && clock.resumedSleeps == activity.wakesHandled
            && sources.values.reduce(0) { $0 + $1.signalsSent } == activity.signalsReceived
            && emitted.get().count == activity.eventsEmitted
            && drainChecks.get().allSatisfy { $0() }
    }

    /// Waits until the engine is quiescent (see `isQuiescent`) on three consecutive checks. Every wait for "nothing
    /// more happens" uses this, so it never depends on how many yields the scheduler needed.
    func settle(sourceLocation: SourceLocation = #_sourceLocation) async {
        var stable = 0
        await poll("quiescence", dump: { await self.dump() }, sourceLocation: sourceLocation) {
            stable = isQuiescent ? stable + 1 : 0
            return stable >= 3
        }
    }

    /// The engine's bookkeeping and state, for failure messages.
    func dump() async -> String {
        let state = await engine.currentState()
        let signals = sources.values.reduce(0) { $0 + $1.signalsSent }
        return "[activity: \(engine.activity.summary); clock resumed=\(clock.resumedSleeps) pending=\(clock.pendingSleeps); "
            + "signals sent=\(signals); events captured=\(emitted.get().count); revision=\(state.revision); "
            + "statuses=\(state.statuses); events=\(state.events.map(\.sourceEventID))]"
    }

    var widgetSnapshots: [WidgetSnapshot] {
        emitted.get().compactMap { if case .widgetSnapshot(let snapshot) = $0 { snapshot } else { nil } }
    }

    var dedupFile: URL { directory.appendingPathComponent("dedup-state.json") }

    var takeoverDueIDs: [String] {
        emitted.get().compactMap { if case .takeoverDue(let request, _) = $0 { request.event.sourceEventID } else { nil } }
    }
}

/// A host text model for engine tests: answers every prompt with `answer` (or throws), counts calls and can run a
/// hook on each call.
final class FakeEngineGenerator: TextGenerator, @unchecked Sendable {
    struct Failure: Error {}
    static let onDevice = EngineInfo(id: "fake-model", displayName: "Fake model", isOnDevice: true)

    let availabilityValue: LockedBox<TextGeneratorAvailability>
    let answer: LockedBox<String?>
    let calls = LockedBox(0)
    /// Runs with the 1-based call number before the call answers.
    let onCall = LockedBox<(@Sendable (Int) -> Void)?>(nil)

    /// `answer` nil makes every call throw `Failure`.
    init(_ availability: TextGeneratorAvailability = .available(FakeEngineGenerator.onDevice), answer: String? = "different") {
        availabilityValue = LockedBox(availability)
        self.answer = LockedBox(answer)
    }

    let availabilityReads = LockedBox(0)
    /// When set, the availability read with this 1-based number waits at the gate first.
    let availabilityGate = LockedBox<(read: Int, gate: Gate)?>(nil)

    var availability: TextGeneratorAvailability {
        get async {
            let read = availabilityReads.mutate { $0 += 1; return $0 }
            if let armed = availabilityGate.get(), armed.read == read { await armed.gate.wait() }
            return availabilityValue.get()
        }
    }

    func generate(instructions: String, prompt: String) async throws -> String {
        let number = calls.mutate { $0 += 1; return $0 }
        onCall.get()?(number)
        guard let answer = answer.get() else { throw Failure() }
        return answer
    }
}
