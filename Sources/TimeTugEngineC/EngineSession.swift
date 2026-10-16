import CalendarCore
import Foundation
import TimeTugCore
import TimeTugEngine
import TimeTugEngineProtocol

/// The engine's version as reported in `ready`. The release workflow stamps the release tag here before it builds the
/// package; the repository keeps the development value.
enum EngineBuild {
    static let version = "0.0.0-dev"
}

/// One engine behind the C interface: the `Engine`, its remote host services, the command intake and the delivery
/// to the host. Created by `tt_engine_create`, ended by `tt_engine_destroy` (`shutdown`).
///
/// Commands: `send` only enqueues (never blocks). One intake task decodes them in order against the latest published
/// state and hands them to lanes, as the Mac app's command queue does: commands in the same lane reach the engine one
/// after another in send order, and lanes do not wait for each other, so a sign-in waiting for a person never holds
/// up a takeover. `account.cancel` and `test.advanceClock` act at once, because the work they end or unblock may
/// itself be what a lane is waiting on.
///
/// Messages: `ready` first (on the first `start`), then states from the state pump and events from the event pump,
/// through `Outbox` and `Delivery`. Engine diagnostics reach the host through the engine host's diagnostic sink,
/// which sees every diagnostic, including the persistence stores' that never become `.diagnostic` events; the event
/// pump skips `.diagnostic` so none is delivered twice.
final class EngineSession: @unchecked Sendable {
    private enum Lane: Hashable {
        case general, accounts, settings, countdown, takeover, dedup
    }

    let config: WireConfig
    let requests: RequestTable
    private let engine: Engine
    private let manualClock: ManualEngineClock?
    private let zone: ZoneOverride
    private let decoder: CommandDecoder
    private let delivery: Delivery
    private let outbox: Outbox
    private let intake: AsyncStream<String>.Continuation

    private let lock = NSLock()
    private var latestState = EngineState.empty
    private var started = false
    private var stopped = false
    private var lanes: [Lane: AsyncStream<EngineCommand>.Continuation] = [:]
    private var tasks: [Task<Void, Never>] = []

    /// Parses and validates `configJSON`, creates the data directory and builds the engine (not started).
    /// Returns nil for any invalid config; nothing is delivered then.
    static func make(configJSON: String, callback: HostCallback) -> EngineSession? {
        guard let config = try? WireConfig.parse(configJSON) else { return nil }
        let directory = URL(fileURLWithPath: config.dataDirectory, isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        } catch {
            return nil
        }
        return EngineSession(config: config, directory: directory, callback: callback)
    }

    private init(config: WireConfig, directory: URL, callback: HostCallback) {
        self.config = config
        let delivery = Delivery(callback: callback)
        let outbox = Outbox(delivery: delivery)
        self.delivery = delivery
        self.outbox = outbox

        let clock: any EngineClock
        if config.testing {
            let start = config.testClockStart.flatMap(WireDates.date) ?? Date()
            let manual = ManualEngineClock(start: start)
            manualClock = manual
            clock = manual
        } else {
            manualClock = nil
            clock = SystemClock()
        }
        let zone = ZoneOverride(config.timeZone.flatMap(TimeZone.init(identifier:)))
        self.zone = zone
        decoder = CommandDecoder(testing: config.testing)

        let requests = RequestTable(send: { outbox.post(Self.encode($0)) }, clock: clock)
        self.requests = requests
        let diagnostics: EngineDiagnosticSink = { outbox.post(Self.encode(wireDiagnostic($0))) }
        let connectors = ConnectorSetup.make(
            config: config, transport: RemoteHTTPTransport(table: requests), clock: clock, defaultZone: zone.current)
        let host = EngineHost(
            dataDirectory: directory,
            settings: FileSettingsStorage(directory: directory, diagnostics: diagnostics),
            credentials: RemoteCredentialStore(table: requests),
            interaction: RemoteAuthorizationInteraction(table: requests),
            signIn: RemoteSignInPrompting(table: requests),
            registry: connectors.registry,
            unconfiguredKindIDs: connectors.unconfiguredKindIDs,
            textGenerator: config.textGeneration ? RemoteTextGenerator(table: requests) : nil,
            diagnostics: diagnostics,
            clock: clock,
            calendar: {
                var calendar = Calendar.current
                calendar.timeZone = zone.current
                return calendar
            })
        engine = Engine(host: host)

        let (stream, continuation) = AsyncStream.makeStream(of: String.self, bufferingPolicy: .unbounded)
        intake = continuation
        delivery.start()
        let intakeTask = Task { [self] in
            for await text in stream {
                await handle(text)
            }
        }
        lock.withLock { tasks.append(intakeTask) }
    }

    // MARK: - From the host

    /// Enqueues one command; never blocks. Dropped after `shutdown`.
    func send(_ json: String) {
        intake.yield(json)
    }

    func reply(requestID: UInt64, json: String) {
        requests.fulfill(id: requestID, reply: json)
    }

    /// Stops the engine. From outside this engine's callback it waits until no callback runs (none starts after it
    /// returns) and, bounded, until the engine has stopped, so the data directory is free for a new engine. From inside
    /// a callback it only cancels and returns at once. Idempotent.
    func shutdown() {
        let alreadyStopped = lock.withLock { () -> Bool in
            defer { stopped = true }
            return stopped
        }
        guard !alreadyStopped else { return }
        let fromCallback = delivery.isDeliveryThread
        intake.finish()
        delivery.close(waitForCallback: !fromCallback)
        requests.failAll(kind: .cancelled)
        let (tasks, lanes) = lock.withLock { () -> ([Task<Void, Never>], [AsyncStream<EngineCommand>.Continuation]) in
            defer {
                self.tasks = []
                self.lanes = [:]
            }
            return (self.tasks, Array(self.lanes.values))
        }
        for lane in lanes { lane.finish() }
        for task in tasks { task.cancel() }
        let engine = engine
        let stoppedSignal = Signal()
        Task {
            await engine.send(.stop)
            stoppedSignal.fire()
        }
        if !fromCallback { stoppedSignal.wait(timeout: 10) }
    }

    // MARK: - Command intake (one task, in order)

    private func handle(_ text: String) async {
        guard !isStopped else { return }
        let envelope: Envelope
        do {
            envelope = try JSONDecoder().decode(Envelope.self, from: Data(text.utf8))
        } catch {
            let id = (try? JSONDecoder().decode(IDOnly.self, from: Data(text.utf8)))?.id
            reject(id: id, message: (try? JSONDecoder().decode(JSONValue.self, from: Data(text.utf8))) == nil
                   ? "invalid JSON" : "invalid envelope")
            return
        }
        switch decoder.decode(envelope, state: lock.withLock { latestState }) {
        case .rejected(let diagnostic):
            outbox.post(Self.encode(wireDiagnostic(diagnostic, id: envelope.id)))
        case .testAdvanceClock(let seconds):
            manualClock?.advance(by: seconds)
        case .timeZone(let newZone, let command):
            zone.set(newZone)
            dispatch(command)
        case .engine(.start):
            await startOnce()
            dispatch(.start)
        case .engine(.cancelAuthorization):
            let engine = engine
            track(Task { await engine.send(.cancelAuthorization) })
        case .engine(let command):
            dispatch(command)
        }
    }

    private struct IDOnly: Decodable { var id: MessageID? }

    private func reject(id: MessageID?, message: String) {
        let diagnostic = EngineDiagnostic(level: .warning, category: "command", message: message)
        outbox.post(Self.encode(wireDiagnostic(diagnostic, id: id)))
    }

    /// The first `start` publishes `ready` and subscribes the pumps before the engine starts, so no state or event of
    /// the start is missed. A later `start` goes to the engine alone (which refuses it with a diagnostic).
    private func startOnce() async {
        guard lock.withLock({ () -> Bool in
            defer { started = true }
            return !started
        }) else { return }
        let ready = WireReady(
            engineVersion: EngineBuild.version, platform: config.platform,
            capabilities: .init(systemCalendar: false, textGeneration: config.textGeneration))
        outbox.openWith(ready: Self.encode(Envelope(type: "ready", payload: JSONValue(encoding: ready))))
        let states = await engine.states
        let events = await engine.events
        let outbox = outbox
        track(Task { [weak self] in
            for await state in states {
                guard let self else { return }
                self.lock.withLock { self.latestState = state }
                outbox.postState(Self.encode(wireMessage(for: state)))
            }
        })
        track(Task {
            for await event in events {
                if case .diagnostic = event { continue }
                outbox.post(Self.encode(wireMessage(for: event)))
            }
        })
    }

    private func lane(for command: EngineCommand) -> Lane {
        switch command {
        case .start, .refresh, .system, .stop, .settingsChangedExternally: .general
        case .addAccount, .reauthorizeAccount, .removeAccount, .cancelAuthorization: .accounts
        case .applySettings: .settings
        case .setCountdownVisible: .countdown
        case .takeoverPresented, .takeoverClosed, .join, .snooze, .dismiss, .testTug: .takeover
        case .merge, .unmerge, .separate, .forgetCorrections: .dedup
        }
    }

    private func dispatch(_ command: EngineCommand) {
        let lane = lane(for: command)
        let engine = engine
        let continuation: AsyncStream<EngineCommand>.Continuation? = lock.withLock {
            guard !stopped else { return nil }
            if let existing = lanes[lane] { return existing }
            let (stream, continuation) = AsyncStream.makeStream(of: EngineCommand.self, bufferingPolicy: .unbounded)
            lanes[lane] = continuation
            tasks.append(Task {
                for await command in stream { await engine.send(command) }
            })
            return continuation
        }
        continuation?.yield(command)
    }

    private func track(_ task: Task<Void, Never>) {
        let keep = lock.withLock { () -> Bool in
            guard !stopped else { return false }
            tasks.removeAll { $0.isCancelled }
            tasks.append(task)
            return true
        }
        if !keep { task.cancel() }
    }

    private var isStopped: Bool { lock.withLock { stopped } }

    static func encode(_ envelope: Envelope) -> String {
        // Encoding the module's own DTOs cannot fail; an empty object would still be valid JSON for the host.
        guard let data = try? JSONEncoder().encode(envelope) else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }
}

/// The time zone the engine's calendar uses: the config's or a `timeZoneChanged` zone, else the process zone.
final class ZoneOverride: @unchecked Sendable {
    private let lock = NSLock()
    private var zone: TimeZone?

    init(_ zone: TimeZone?) { self.zone = zone }

    var current: TimeZone { lock.withLock { zone } ?? TimeZone.current }
    func set(_ zone: TimeZone) { lock.withLock { self.zone = zone } }
}

/// A one-shot signal a thread can wait on with a timeout.
final class Signal: @unchecked Sendable {
    private let condition = NSCondition()
    private var fired = false

    func fire() {
        condition.lock()
        fired = true
        condition.broadcast()
        condition.unlock()
    }

    /// Returns true when fired, false after `timeout` seconds.
    @discardableResult
    func wait(timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        condition.lock()
        defer { condition.unlock() }
        while !fired {
            if !condition.wait(until: deadline) { return fired }
        }
        return true
    }
}
