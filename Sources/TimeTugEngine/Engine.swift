import CalendarBridge
import CalendarCore
import Foundation
import TimeTugCore

/// The platform-neutral "what and when" of TimeTug: sources, the merged calendar, the takeover ledger and the state
/// every front end renders. Front ends send `EngineCommand`s and observe `states` and `events`; presentation is
/// theirs. Ported from the Mac app's `AppCoordinator`.
///
/// Reentrancy: every `await` inside the actor (store refreshes, account calls) lets other commands and
/// publications run. `apply(_:)` is synchronous and guards on `CalendarSnapshot.revision`, so a late publication
/// from an older refresh is dropped and the published `EngineState.revision` only ever increases.
public actor Engine {
    /// Backstop only: change signals (source polls, wake, network return) drive the real refreshes.
    static let periodicRefresh: TimeInterval = 900
    /// Meetings that started longer than this before launch are treated as already handled.
    static let launchGrace: TimeInterval = 120
    /// Bounded backstop for inference resolution (Task 10).
    static let maxResolvePasses = 10

    private let host: EngineHost
    private let store: CalendarStore
    private let adjudicator: TextGeneratorAdjudicator?
    private let ledgerStore: LedgerStore
    private let dedupStore: DedupStateStore
    private let accountManager: AccountManager
    private let reconciler: SourceReconciler
    private let refreshCoordinator: SourceRefreshCoordinator

    // Model
    private var settings: EngineSettings
    private var ledger = TakeoverLedger()
    private var launchAcknowledgement: LaunchAcknowledgement
    private var snapshot = CalendarSnapshot.empty
    private var agenda = DayAgenda.empty
    private var accountStatus = AccountManager.Status.empty
    private var inferenceStatus = InferenceStatus.disabled
    private var lastSavedDedupState: DedupState?

    // Takeover scheduling
    private var nextTakeover: ScheduledTakeover?
    /// As reported by the host (`takeoverPresented`), on screen or as a toast.
    private var takeoverVisible = false
    private var fireTask: Task<Void, Never>?
    /// Bumped on every re-arm; a sleep from an older arming that still wakes does nothing.
    private var fireGeneration: UInt64 = 0
    /// The takeover emitted and not yet closed (the plan's `pendingTakeoverEventID`); commands name it by `id`.
    private var outstandingTakeover: TimeTugCalendarEvent?
    /// True while the outstanding takeover is the test tug's sample.
    private var outstandingIsTest = false

    // Background work
    private var started = false
    private var periodicTask: Task<Void, Never>?
    /// Inference resolution off the refresh path (Task 10).
    private var resolveTask: Task<Void, Never>?
    /// UI tick for countdowns and agenda boundaries (Task 10).
    private var tickTask: Task<Void, Never>?
    /// The running sign-in (add or reauthorize); `cancelAuthorization` cancels it.
    private var authTask: Task<Void, Never>?

    // Publication
    private var revision: UInt64 = 0
    private var state: EngineState
    private var stateContinuations: [UUID: AsyncStream<EngineState>.Continuation] = [:]
    private var eventContinuations: [UUID: AsyncStream<EngineEvent>.Continuation] = [:]

    public init(host: EngineHost) {
        self.host = host
        let settings = host.settings.load()
        self.settings = settings
        launchAcknowledgement = LaunchAcknowledgement(launchedAt: host.clock.now())
        ledgerStore = LedgerStore(directory: host.dataDirectory, diagnostics: host.diagnostics)
        dedupStore = DedupStateStore(directory: host.dataDirectory, diagnostics: host.diagnostics)
        let adjudicator = host.textGenerator.map { generator in
            TextGeneratorAdjudicator(
                generator: generator, clock: host.clock, timeZone: { host.calendar().timeZone },
                diagnostics: host.diagnostics)
        }
        self.adjudicator = adjudicator
        let clock = host.clock
        store = CalendarStore(sources: [], calendar: host.calendar(), adjudicator: adjudicator,
                              passClock: { clock.now() })

        // The trio below calls back into the engine through `ref`, which is filled once `self` exists.
        let ref = EngineRef()
        let coordinator = SourceRefreshCoordinator { ids in await ref.engine?.refresh(sourceIDs: ids) }
        refreshCoordinator = coordinator
        let registry = host.registry, credentials = host.credentials
        let syncState = FileSyncStateStore(url: host.dataDirectory.appendingPathComponent("sync-state.json"))
        let systemSource = host.buildSystemSource
        let reconciler = SourceReconciler(
            buildAccount: { connection in
                guard let kind = registry.kind(id: connection.kindID) else {
                    throw CalendarCore.SourceError.invalidResponse("unknown account type")
                }
                return ConnectedSource(try kind.makeSource(for: connection, credentials: credentials, syncState: syncState))
            },
            // Never called without a host source: `effectiveAccountSettings` reports the switch off then.
            buildSystemSource: { systemSource?() ?? NoSystemSource() },
            onChange: { change in await coordinator.signal(change) })
        self.reconciler = reconciler
        accountManager = AccountManager(
            host: host, reconciler: reconciler, syncState: syncState,
            settings: { await ref.engine?.effectiveAccountSettings() ?? EngineSettings(systemCalendarEnabled: false) },
            applySources: { sources in await ref.engine?.applySources(sources) },
            updateSettings: { change in await ref.engine?.updateSettings(change) })

        state = EngineState.empty
        state.settings = settings
        ref.engine = self
    }

    // MARK: - Public API

    /// The latest state. Each new subscriber first receives the current state; a slow subscriber sees only the newest.
    public var states: AsyncStream<EngineState> {
        let (stream, continuation) = AsyncStream.makeStream(of: EngineState.self, bufferingPolicy: .bufferingNewest(1))
        let id = UUID()
        continuation.yield(state)
        stateContinuations[id] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeSubscriber(id) }
        }
        return stream
    }

    /// One-shot notifications, buffered without limit. Every subscriber receives events emitted after it subscribed.
    public var events: AsyncStream<EngineEvent> {
        let (stream, continuation) = AsyncStream.makeStream(of: EngineEvent.self, bufferingPolicy: .unbounded)
        let id = UUID()
        eventContinuations[id] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeSubscriber(id) }
        }
        return stream
    }

    public func currentState() -> EngineState { state }

    /// Loads the persisted state, builds the sources and refreshes once. Returns after that first refresh.
    public func start() async {
        guard !started else {
            diagnose(.warning, "engine", "start was sent more than once; ignored")
            return
        }
        started = true
        let now = host.clock.now()
        ledger = ledgerStore.load()
        ledger.prune(now: now)
        diagnose(.info, "ledger", "ledger loaded with \(ledger.keyCount) keys")
        persistLedger()
        let dedup = dedupStore.load()
        lastSavedDedupState = dedup
        await store.load(dedup)
        if let adjudicator { await adjudicator.refreshAvailability() }
        _ = await store.setInferenceEnabled(settings.inferenceEnabled, now: host.clock.now())
        inferenceStatus = await store.inferenceStatus()
        armPeriodicRefresh()
        // Requests system calendar access if enabled, builds the sources and refreshes (through `applySources`).
        await accountManager.start()
        await accountsChanged()
    }

    public func send(_ command: EngineCommand) async {
        switch command {
        case .start: await start()
        case .refresh: await refresh(sourceIDs: nil)
        case .addAccount(let kindID): await runAuthorization { await $0.addAccount(kindID: kindID) }
        case .removeAccount(let id):
            await accountManager.removeAccount(connectionID: id)
            await accountsChanged()
        case .reauthorizeAccount(let id): await runAuthorization { await $0.reauthorize(connectionID: id) }
        case .cancelAuthorization:
            authTask?.cancel()
            await accountManager.cancelAuthorization()
        case .applySettings(let change): await applySettings(change)
        case .settingsChangedExternally: await settingsChangedExternally()
        case .setCountdownVisible(let visible): setCountdownVisible(visible)
        case .takeoverPresented(let eventID, let onScreen): takeoverPresented(eventID: eventID, onScreen: onScreen)
        case .takeoverClosed(let eventID): takeoverClosed(eventID: eventID)
        case .join(let eventID): join(eventID: eventID)
        case .snooze(let eventID, let seconds): snooze(eventID: eventID, seconds: seconds)
        case .dismiss(let eventID): dismiss(eventID: eventID)
        case .testTug: testTug()
        case .merge(let a, let b): await merge(a, b)
        case .unmerge(let event): await unmerge(event)
        case .separate(let event, let members): await separate(event, members: members)
        case .forgetCorrections: await forgetCorrections()
        case .system(let event): await handleSystemEvent(event)
        case .stop: await stop()
        }
    }

    // MARK: - Refresh and apply

    private func refresh(sourceIDs: Set<String>?) async {
        let result = await store.refresh(
            now: host.clock.now(), leadTime: settings.takeover.leadTime, sourceIDs: sourceIDs,
            onPublication: { [weak self] snapshot in await self?.apply(snapshot) })
        apply(result)
        // Inference never blocks a refresh.
        startInferenceResolution()
    }

    /// Publishes a snapshot: ledger bookkeeping, launch acknowledgement, re-arm, state and widget snapshot.
    /// A snapshot no newer than the current one is dropped (a late publication from an older refresh).
    private func apply(_ newSnapshot: CalendarSnapshot) {
        guard newSnapshot.revision > snapshot.revision else { return }
        snapshot = newSnapshot
        let now = host.clock.now()
        if ledger.prune(now: now) { persistLedger() }
        let toAcknowledge = launchAcknowledgement.eventsToAcknowledge(in: snapshot, now: now, grace: Self.launchGrace)
        if !toAcknowledge.isEmpty {
            for event in toAcknowledge { ledger.markFired(event, now: now) }
            diagnose(.info, "takeover", "acknowledged \(toAcknowledge.count) meeting(s) already underway at launch")
            persistLedger()
        }
        rearm()
        publish()
        emitWidgetSnapshot()
    }

    /// Called by the account manager whenever the source set changes.
    private func applySources(_ sources: [any TimeTugCore.CalendarSource]) async {
        launchAcknowledgement.registerSources(Set(sources.map(\.id)), now: host.clock.now())
        accountStatus = await accountManager.status()
        await store.setSources(sources)
        await refresh(sourceIDs: nil)
    }

    private func armPeriodicRefresh() {
        periodicTask?.cancel()
        let clock = host.clock
        periodicTask = Task { [weak self] in
            var deadline = clock.now().addingTimeInterval(Self.periodicRefresh)
            while !Task.isCancelled {
                do { try await clock.sleep(until: deadline) } catch { return }
                guard let self else { return }
                await self.refresh(sourceIDs: nil)
                deadline = clock.now().addingTimeInterval(Self.periodicRefresh)
            }
        }
    }

    // MARK: - Accounts

    /// Runs one sign-in in an engine-owned task so `cancelAuthorization` can cancel it; returns when it ends.
    private func runAuthorization(_ body: @escaping @Sendable (AccountManager) async -> Void) async {
        guard authTask == nil else {
            diagnose(.warning, "accounts", "a sign-in is already running; ignored")
            return
        }
        let manager = accountManager
        let task = Task { await body(manager) }
        authTask = task
        await task.value
        authTask = nil
        await accountsChanged()
    }

    /// Re-reads the account manager's status and publishes it.
    private func accountsChanged() async {
        accountStatus = await accountManager.status()
        publish()
    }

    /// The settings the account manager sees: the system calendar switch reads as off when the host has no system source.
    private func effectiveAccountSettings() -> EngineSettings {
        var effective = settings
        if host.buildSystemSource == nil { effective.systemCalendarEnabled = false }
        return effective
    }

    /// Changes, persists and publishes the behaviour settings. Task 10's `applySettings` builds on this.
    private func updateSettings(_ change: @Sendable (inout EngineSettings) -> Void) {
        var updated = settings
        change(&updated)
        guard updated != settings else { return }
        settings = updated
        host.settings.save(updated)
        rearm()
        publish()
        emitWidgetSnapshot()
    }

    // MARK: - Takeover scheduling

    /// True from the moment a takeover is emitted until the host closes it, and while the host reports one visible
    /// (on screen or as a toast). The host reports presentation asynchronously, so the emitted-but-unreported
    /// window counts too: a second takeover waits for the first to close.
    private var takeoverOutstanding: Bool { takeoverVisible || outstandingTakeover != nil }

    /// Arms one sleep for the next takeover (port of the Mac `rearm`). Skipped while a takeover is outstanding;
    /// closing it re-arms. Emits `takeoverWithdrawn` when the previously armed meeting is no longer scheduled at all.
    private func rearm() {
        fireTask?.cancel()
        fireTask = nil
        fireGeneration &+= 1
        let previous = nextTakeover
        nextTakeover = nil
        guard !takeoverOutstanding else { return }
        let now = host.clock.now()
        let next = Scheduler.next(events: snapshot.events, settings: settings.takeover, ledger: ledger, now: now)
        nextTakeover = next
        if let previous, !isScheduled(previous.event, now: now) {
            emit(.takeoverWithdrawn(eventID: previous.event.id))
        }
        guard let next else { return }
        let clock = host.clock, generation = fireGeneration
        fireTask = Task { [weak self] in
            do { try await clock.sleep(until: next.fireAt) } catch { return }
            await self?.fireArmed(next.event, generation: generation)
        }
    }

    /// Whether the current copy of `event` would still be armed at some point.
    private func isScheduled(_ event: TimeTugCalendarEvent, now: Date) -> Bool {
        guard let current = snapshot.events.first(where: { $0.isSameMeeting(as: event) }) else { return false }
        return Scheduler.next(events: [current], settings: settings.takeover, ledger: ledger, now: now) != nil
    }

    /// Runs `fire` only for the sleep armed last: a re-armed (cancelled) sleep that already woke does nothing,
    /// as an invalidated Mac timer never fired.
    private func fireArmed(_ event: TimeTugCalendarEvent, generation: UInt64) {
        guard generation == fireGeneration else { return }
        fireTask = nil
        fire(event)
    }

    /// Re-checks the ledger and the current snapshot before anything is emitted (port of the Mac `fire`): the event
    /// captured when the sleep was armed may be stale. Diagnostics carry the reason only, never the title.
    private func fire(_ event: TimeTugCalendarEvent) {
        let now = host.clock.now()
        let decision = TakeoverGuard.evaluate(
            event: event, currentEvents: snapshot.events, settings: settings.takeover,
            ledger: ledger, now: now, overlayVisible: takeoverOutstanding)
        switch decision {
        case .suppress(.overlayVisible):
            // Stays pending, not marked fired; closing the visible takeover re-arms.
            diagnose(.info, "takeover", "takeover suppressed: \(TakeoverGuard.Reason.overlayVisible.rawValue)")
        case .suppress(let reason):
            diagnose(.info, "takeover", "takeover suppressed: \(reason.rawValue)")
            nextTakeover = nil
            rearm()
            publish()
        case .present:
            let current = snapshot.events.first { $0.isSameMeeting(as: event) } ?? event
            let reason = ledger.isSnoozed(current) ? "snooze expired"
                : now >= current.start.addingTimeInterval(Self.launchGrace) ? "late after wake" : "lead time"
            ledger.markFired(current, now: now)
            persistLedger()
            diagnose(.info, "takeover", "takeover due (\(reason))")
            nextTakeover = nil
            outstandingTakeover = current
            outstandingIsTest = false
            let calendarTitle = snapshot.calendars.first { $0.key == current.calendarKey }?.title
            emit(.takeoverDue(TakeoverRequest.make(for: current, now: now), calendarTitle: calendarTitle))
            rearm()
            publish()
        }
    }

    /// The host shows the takeover on screen, or (`onScreen` false) as a toast; either holds the next takeover.
    private func takeoverPresented(eventID: String, onScreen: Bool) {
        if let outstandingTakeover, outstandingTakeover.id != eventID {
            diagnose(.warning, "takeover", "presented a takeover that is not the outstanding one")
        }
        takeoverVisible = true
        diagnose(.info, "takeover", onScreen ? "takeover shown on screen" : "takeover shown as a toast")
        publish()
    }

    private func takeoverClosed(eventID: String) {
        guard matchesOutstanding(eventID, command: "takeoverClosed") else { return }
        closeTakeover()
    }

    /// Asks the host to open the meeting link (never for the test tug, as on the Mac), then closes.
    private func join(eventID: String) {
        guard matchesOutstanding(eventID, command: "join") else { return }
        if !outstandingIsTest, let url = outstandingTakeover?.conferenceURL { emit(.joinRequested(url)) }
        closeTakeover()
    }

    /// Re-arms the meeting `seconds` from now (never past its end) and closes. The test tug never touches the ledger.
    private func snooze(eventID: String, seconds: TimeInterval) {
        guard matchesOutstanding(eventID, command: "snooze") else { return }
        if !outstandingIsTest, let event = outstandingTakeover {
            ledger.snooze(event, for: seconds, now: host.clock.now())
        }
        closeTakeover()
    }

    private func dismiss(eventID: String) {
        guard matchesOutstanding(eventID, command: "dismiss") else { return }
        closeTakeover()
    }

    /// A command for another event than the outstanding takeover is stale (a late close of an earlier takeover)
    /// and is ignored so it cannot release the guard held for the current one.
    private func matchesOutstanding(_ eventID: String, command: String) -> Bool {
        guard let outstandingTakeover, outstandingTakeover.id != eventID else { return true }
        diagnose(.warning, "takeover", "\(command) for an event that is not the outstanding takeover; ignored")
        return false
    }

    /// Port of the Mac `closeOverlay`: clears the guard, persists, re-arms and publishes.
    private func closeTakeover() {
        let wasTest = outstandingIsTest
        takeoverVisible = false
        outstandingTakeover = nil
        outstandingIsTest = false
        if !wasTest { persistLedger() }
        rearm()
        publish()
    }

    /// Settings' "Test tug" (port of the Mac `fireTest`): a sample event that never touches the ledger and, as on the
    /// Mac, leaves the armed takeover alone (the guard holds it while the sample is outstanding).
    private func testTug() {
        let now = host.clock.now()
        let leadTime = settings.takeover.leadTime
        var sample = TimeTugCalendarEvent(
            event: CalendarCore.CalendarEvent(
                eventID: "test", calendarID: "test", title: "Sample meeting",
                start: now.addingTimeInterval(leadTime), end: now.addingTimeInterval(leadTime + 1800)),
            sourceID: "test")
        sample.otherAttendeeCount = 1
        sample.conferences = [ConferenceInfo(url: URL(string: "https://meet.google.com/aaa-bbbb-ccc")!, provider: .meet)]
        outstandingTakeover = sample
        outstandingIsTest = true
        diagnose(.info, "takeover", "test takeover due")
        emit(.takeoverDue(TakeoverRequest.make(for: sample, now: now), calendarTitle: "Sample calendar"))
        publish()
    }

    // MARK: - Inference, dedup, settings, system events, ticks (Task 10)

    /// Starts inference resolution in `resolveTask` without blocking the caller. Task 10 implements it.
    private func startInferenceResolution() {}

    /// Persists the change and re-arms (a changed lead time re-arms; `enabled = false` withdraws the armed takeover).
    /// Task 10 adds inference and system calendar handling.
    private func applySettings(_ change: @escaping @Sendable (inout EngineSettings) -> Void) async {
        updateSettings(change)
    }
    private func settingsChangedExternally() async { notImplemented("settingsChangedExternally") }
    private func setCountdownVisible(_ visible: Bool) { notImplemented("setCountdownVisible") }
    private func merge(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) async { notImplemented("merge") }
    private func unmerge(_ event: TimeTugCalendarEvent) async { notImplemented("unmerge") }
    private func separate(_ event: TimeTugCalendarEvent, members: [MergedMember]) async { notImplemented("separate") }
    private func forgetCorrections() async { notImplemented("forgetCorrections") }
    private func handleSystemEvent(_ event: SystemEvent) async { notImplemented("system") }

    // MARK: - Stop

    /// Cancels background work, stops the sources and ends every stream.
    private func stop() async {
        periodicTask?.cancel()
        fireTask?.cancel()
        resolveTask?.cancel()
        tickTask?.cancel()
        authTask?.cancel()
        periodicTask = nil
        fireTask = nil
        resolveTask = nil
        tickTask = nil
        await accountManager.stop()
        for continuation in stateContinuations.values { continuation.finish() }
        for continuation in eventContinuations.values { continuation.finish() }
        stateContinuations = [:]
        eventContinuations = [:]
    }

    // MARK: - Publication

    /// Builds and yields a new state. `rebuildAgenda` false keeps the agenda (a countdown tick, Task 10).
    private func publish(rebuildAgenda: Bool = true) {
        if rebuildAgenda {
            agenda = DayAgenda.make(events: snapshot.events, settings: settings.takeover,
                                    now: host.clock.now(), calendar: host.calendar())
        }
        revision &+= 1
        state = EngineState(
            revision: revision, agenda: agenda, events: snapshot.events, calendars: snapshot.calendars,
            statuses: snapshot.statuses, notices: snapshot.notices, sourceNames: snapshot.sourceNames,
            candidates: snapshot.candidates, accounts: accountStatus.accounts, accountStatus: accountStatus,
            settings: settings, inferenceStatus: inferenceStatus, ledger: ledger, nextTakeover: nextTakeover,
            takeoverVisible: takeoverVisible)
        for continuation in stateContinuations.values { continuation.yield(state) }
    }

    private func emitWidgetSnapshot() {
        emit(.widgetSnapshot(WidgetSnapshot.make(
            events: snapshot.widgetEvents, calendars: snapshot.calendars, settings: settings.takeover,
            now: host.clock.now(), calendar: host.calendar())))
    }

    private func emit(_ event: EngineEvent) {
        for continuation in eventContinuations.values { continuation.yield(event) }
    }

    private func removeSubscriber(_ id: UUID) {
        stateContinuations[id] = nil
        eventContinuations[id] = nil
    }

    private func persistLedger() {
        ledgerStore.save(ledger, now: host.clock.now())
    }

    /// Engine-level diagnostics go to the host sink and, as `.diagnostic`, to `events`. Never include titles.
    private func diagnose(_ level: EngineDiagnostic.Level, _ category: String, _ message: String) {
        let diagnostic = EngineDiagnostic(level: level, category: category, message: message)
        host.diagnostics(diagnostic)
        emit(.diagnostic(diagnostic))
    }

    private func notImplemented(_ command: String) {
        diagnose(.warning, "engine", "command \(command) is not implemented yet")
    }
}

/// Lets the engine's helpers, built in `init` before `self` exists, call back into it without a retain cycle.
private final class EngineRef: @unchecked Sendable {
    private let lock = NSLock()
    private weak var _engine: Engine?
    var engine: Engine? {
        get { lock.withLock { _engine } }
        set { lock.withLock { _engine = newValue } }
    }
}

/// Placeholder for a host without a system calendar source; never reached (see `effectiveAccountSettings`).
private struct NoSystemSource: TimeTugCore.CalendarSource {
    var id: String { "eventkit" }
    var displayName: String { "" }
    func calendars() async throws -> [CalendarInfo] { [] }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] { [] }
    func changes() -> AsyncStream<SourceChange> { AsyncStream { $0.finish() } }
}
