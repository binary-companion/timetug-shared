import Foundation
import TimeTugKitGenerated

/// The engine with the stream accessors of the 1.x `Engine` actor: `states` yields full `EngineState`s and `events` the
/// one-shot events, so a model that read `engine.states` keeps its shape.
///
/// The engine delivers a light `EngineStateSummary` (the three large parts of a state, its events, agenda and merge
/// candidates, carry a revision each). This wrapper fetches a part only when its revision changed since the last state and
/// hands out the assembled state. Every `states` and `events` call makes a new stream; a stream that is created later still
/// starts with the latest state. `states` keeps only the newest value (a slow consumer skips states, as the engine does);
/// `events` buffers without limit.
public final class ObservedEngine: Sendable {
    /// The underlying engine, for the calls this wrapper does not repeat.
    public let engine: Engine
    private let hub: StreamHub

    /// Creates the engine through `make`, which receives the observer to pass to it:
    /// `ObservedEngine { observer in try Engine(config: config, host: host, observer: observer) }`.
    public init(_ make: (EngineObserver) throws -> Engine) rethrows {
        let hub = StreamHub()
        self.hub = hub
        self.engine = try make(hub)
        hub.attach(engine)
    }

    /// The states, starting with the latest one (the empty state before `start()`).
    public var states: AsyncStream<EngineState> { hub.makeStates() }

    /// The events emitted from now on.
    public var events: AsyncStream<EngineEvent> { hub.makeEvents() }

    public func start() async { await engine.start() }

    public func send(_ command: EngineCommand) async { await engine.send(command: command) }

    /// `applySettings { $0.takeover.leadTime = 120 }` against the latest settings.
    public func applySettings(_ change: (inout EngineSettings) -> Void) async {
        await send(.applySettings(from: engine.currentSummary().settings, change))
    }

    /// The latest state in full.
    public func currentState() -> EngineState { engine.currentState() }

    public func setTimeZone(_ identifier: String) throws { try engine.setTimeZone(identifier: identifier) }

    public func shutdown() {
        engine.shutdown()
        hub.finish()
    }
}

/// Fans the engine's observer calls out to any number of streams.
private final class StreamHub: EngineObserver, @unchecked Sendable {
    private let lock = NSLock()
    private weak var engine: Engine?
    private var stateStreams: [UUID: AsyncStream<EngineState>.Continuation] = [:]
    private var eventStreams: [UUID: AsyncStream<EngineEvent>.Continuation] = [:]
    private var latest: EngineState?
    private var parts = Parts()
    private var finished = false

    private struct Parts {
        var events: [TimeTugCalendarEvent] = []
        var agenda = DayAgenda(items: [], next: nil)
        var candidates: [String: [TimeTugCalendarEvent]] = [:]
        var eventsRevision: UInt64?
        var agendaRevision: UInt64?
        var candidatesRevision: UInt64?
    }

    func attach(_ engine: Engine) {
        lock.lock()
        self.engine = engine
        lock.unlock()
    }

    func makeStates() -> AsyncStream<EngineState> {
        let id = UUID()
        return AsyncStream(bufferingPolicy: .bufferingNewest(1)) { continuation in
            lock.lock()
            defer { lock.unlock() }
            if finished { continuation.finish(); return }
            stateStreams[id] = continuation
            if let latest { continuation.yield(latest) }
            continuation.onTermination = { [weak self] _ in
                self?.lock.lock()
                self?.stateStreams[id] = nil
                self?.lock.unlock()
            }
        }
    }

    func makeEvents() -> AsyncStream<EngineEvent> {
        let id = UUID()
        return AsyncStream(bufferingPolicy: .unbounded) { continuation in
            lock.lock()
            defer { lock.unlock() }
            if finished { continuation.finish(); return }
            eventStreams[id] = continuation
            continuation.onTermination = { [weak self] _ in
                self?.lock.lock()
                self?.eventStreams[id] = nil
                self?.lock.unlock()
            }
        }
    }

    func finish() {
        lock.lock()
        finished = true
        let states = Array(stateStreams.values)
        let events = Array(eventStreams.values)
        stateStreams = [:]
        eventStreams = [:]
        lock.unlock()
        states.forEach { $0.finish() }
        events.forEach { $0.finish() }
    }

    // MARK: EngineObserver (called on the engine's delivery thread, one call at a time)

    func stateChanged(summary: EngineStateSummary) {
        lock.lock()
        guard let engine, !finished else { lock.unlock(); return }
        var parts = self.parts
        lock.unlock()
        // Fetch outside the lock: these are calls into the engine.
        if parts.eventsRevision != summary.eventsRevision {
            parts.events = engine.currentEvents()
            parts.eventsRevision = summary.eventsRevision
        }
        if parts.agendaRevision != summary.agendaRevision {
            parts.agenda = engine.currentAgenda()
            parts.agendaRevision = summary.agendaRevision
        }
        if parts.candidatesRevision != summary.candidatesRevision {
            parts.candidates = engine.currentCandidates()
            parts.candidatesRevision = summary.candidatesRevision
        }
        let state = EngineState(
            revision: summary.revision, agenda: parts.agenda, events: parts.events, calendars: summary.calendars,
            statuses: summary.statuses, notices: summary.notices, sourceNames: summary.sourceNames,
            candidates: parts.candidates, accounts: summary.accounts, accountStatus: summary.accountStatus,
            settings: summary.settings, inferenceStatus: summary.inferenceStatus, ledger: summary.ledger,
            nextTakeover: summary.nextTakeover, takeoverVisible: summary.takeoverVisible,
            availableAccountKinds: summary.availableAccountKinds)
        lock.lock()
        self.parts = parts
        latest = state
        let streams = Array(stateStreams.values)
        lock.unlock()
        streams.forEach { $0.yield(state) }
    }

    func event(event: EngineEvent) {
        lock.lock()
        let streams = Array(eventStreams.values)
        lock.unlock()
        streams.forEach { $0.yield(event) }
    }
}
