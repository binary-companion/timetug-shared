import CalendarCore
import Foundation
import TimeTugCore

/// What a reconcile produced: the full source set to apply and the connections whose source could not be built.
struct SourceReconcilerUpdate: Sendable {
    let sources: [any TimeTugCore.CalendarSource]
    let failures: [ConnectionID: String]
}

/// What `AccountManager` needs from the source reconciler (Task 4's `SourceReconciler` conforms).
protocol SourceReconciling: Sendable {
    func reconcile(connections: [Connection], systemCalendarEnabled: Bool) async -> SourceReconcilerUpdate
    func rebuild(_ connection: Connection, connections: [Connection], systemCalendarEnabled: Bool) async -> SourceReconcilerUpdate
    func sourceID(forConnection: ConnectionID) async -> String?
    func stop() async
}

/// Owns the user's accounts: add, remove, re-sign-in and the system calendar switch. It persists changes, keeps
/// the source set current through the reconciler and forgets a removed account's calendar selections.
public actor AccountManager {
    public enum WaitingStyle: Equatable, Sendable { case browser, inApp }

    public struct Status: Equatable, Sendable {
        public var accounts: [Connection] = []
        public var isWorking = false
        public var buildFailures: [ConnectionID: String] = [:]
        public var errorMessage: String?
        public var waitingStyle: WaitingStyle = .browser

        public static let empty = Status()
    }

    private let host: EngineHost
    private let reconciler: any SourceReconciling
    private let connectionStore: FileConnectionStore
    private let syncState: any SyncStateStore
    private let settings: @Sendable () async -> EngineSettings
    private let updateSettings: @Sendable (@Sendable (inout EngineSettings) -> Void) async -> Void
    private let applySources: @Sendable ([any TimeTugCore.CalendarSource]) async -> Void
    private let onStatusChange: @Sendable (Status) -> Void
    private let diagnose: EngineDiagnosticSink
    /// Every change (accounts, `isWorking`, `errorMessage`, `buildFailures`, `waitingStyle`) is reported through
    /// `onStatusChange`, so a front end can show sign-in progress while it runs.
    private var current = Status() {
        didSet { if current != oldValue { onStatusChange(current) } }
    }
    private var sourceIDs: [ConnectionID: String] = [:]
    private var authTask: Task<Void, Never>?
    /// Connection ids whose removal is in flight; sign-ins that produce one are discarded, not applied.
    private var removing: Set<ConnectionID> = []

    init(
        host: EngineHost, reconciler: any SourceReconciling,
        syncState: (any SyncStateStore)? = nil,
        settings: @escaping @Sendable () async -> EngineSettings,
        applySources: @escaping @Sendable ([any TimeTugCore.CalendarSource]) async -> Void,
        updateSettings: @escaping @Sendable (@Sendable (inout EngineSettings) -> Void) async -> Void,
        onStatusChange: @escaping @Sendable (Status) -> Void = { _ in },
        diagnostics: EngineDiagnosticSink? = nil
    ) {
        self.host = host
        self.reconciler = reconciler
        self.connectionStore = FileConnectionStore(url: host.dataDirectory.appendingPathComponent("accounts.json"))
        self.syncState = syncState ?? FileSyncStateStore(url: host.dataDirectory.appendingPathComponent("sync-state.json"))
        self.settings = settings
        self.applySources = applySources
        self.updateSettings = updateSettings
        self.onStatusChange = onStatusChange
        self.diagnose = diagnostics ?? host.diagnostics
    }

    public func status() -> Status { current }

    /// Account kinds offered by "add" (system-permission kinds are a switch, not an account). "Other CalDAV" goes last.
    public func availableKindIDs() -> [String] { availableKinds().map(\.id) }

    /// The "add account" choices: the registry's kinds for this platform (system kinds excluded) plus the
    /// `unconfigured` kind ids, which are listed with `isConfigured` false; sorted by id with CalDAV last.
    static func kindInfos(registry: ConnectorRegistry, unconfigured: [String]) -> [AccountKindInfo] {
        var infos: [String: AccountKindInfo] = [:]
        for kind in availableKinds(in: registry) {
            let authorization: AccountKindInfo.Authorization
            switch kind.authorization {
            case .oauth: authorization = .oauth
            case .password: authorization = .password
            case .system: authorization = .system
            }
            infos[kind.id] = AccountKindInfo(
                id: kind.id, displayName: kind.displayName, authorization: authorization, isConfigured: true)
        }
        for id in unconfigured {
            if infos[id] != nil { infos[id]?.isConfigured = false; continue }
            if case .system? = registry.kind(id: id)?.authorization { continue }
            // The engine has no name for a kind this build did not register; the host may localize it by id.
            let name = registry.kind(id: id)?.displayName ?? (id.prefix(1).uppercased() + id.dropFirst())
            infos[id] = AccountKindInfo(id: id, displayName: name, authorization: .oauth, isConfigured: false)
        }
        let sorted = infos.values.sorted { $0.id < $1.id }
        return sorted.filter { $0.id != "caldav" } + sorted.filter { $0.id == "caldav" }
    }

    /// The key of this account's status in `CalendarSnapshot.statuses`.
    public func statusKey(for connection: Connection) -> String {
        sourceIDs[connection.connectionID] ?? connection.sourceID
    }

    public func start() async {
        switch await connectionStore.load() {
        case .loaded(let list):
            current.accounts = list
            await sweepOrphanedSelections()
        case .missing:
            current.accounts = []
            await sweepOrphanedSelections()
        case .unreadable:
            current.accounts = []
            current.errorMessage = "Your saved accounts could not be read, so none were loaded. Nothing was deleted."
        }
        if await settings().systemCalendarEnabled { await host.requestSystemAccess?() }
        await reconcileAndApply()
    }

    public func addAccount(kindID: String) async {
        guard !current.isWorking else { return }
        guard let kind = host.registry.kind(id: kindID) else {
            invalidCommand("Unknown account type.", "addAccount: unknown account type \(kindID)")
            return
        }
        current.isWorking = true
        current.errorMessage = nil
        defer { current.isWorking = false }
        var authorized: Connection?
        do {
            let interaction = host.interaction, credentials = host.credentials
            let connection = try await signIn(with: kind, prefill: [:]) {
                try await kind.authorize(using: interaction, credentials: credentials)
            }
            authorized = connection
            // A cancel that races the end of the sign-in must not leave a half-added account behind.
            try Task.checkCancellation()
            if removing.contains(connection.connectionID) {
                await discardSecretsIfUnowned(connection.connectionID)
                current.errorMessage = "That account is being removed."
                return
            }
            if current.accounts.contains(where: { $0.kindID == connection.kindID && $0.displayName == connection.displayName }) {
                await discardSecretsIfUnowned(connection.connectionID)
                current.errorMessage = "\(connection.displayName) is already added."
                return
            }
            try await connectionStore.add(connection)
            current.accounts.append(connection)
            await reconcileAndApply()
        } catch is CancellationError {
            if let authorized { await discardSecretsIfUnowned(authorized.connectionID) }
        } catch {
            if let authorized { await discardSecretsIfUnowned(authorized.connectionID) }
            current.errorMessage = Self.describe(error)
        }
    }

    /// Deletes a discarded sign-in's secrets, unless a stored account has the same connection id.
    private func discardSecretsIfUnowned(_ connectionID: ConnectionID) async {
        guard !current.accounts.contains(where: { $0.connectionID == connectionID }) else { return }
        try? await host.credentials.removeSecrets(for: connectionID)
    }

    /// Starts `addAccount` as a cancellable task (`cancelAuthorization` stops it).
    public func beginAddAccount(kindID: String) {
        authTask = Task { await self.addAccount(kindID: kindID) }
    }

    public func beginReauthorize(connectionID: ConnectionID) {
        guard !current.isWorking else { return }
        authTask = Task { await self.reauthorize(connectionID: connectionID) }
    }

    public func cancelAuthorization() { authTask?.cancel() }

    /// Runs one sign-in. A `.password` kind signs in through the host's credential form: when a submitted form
    /// fails, the form opens again with the error and what the user typed (never the password), until it succeeds
    /// or the user cancels.
    private func signIn(
        with kind: any ConnectorKind, prefill: [String: String], _ attempt: () async throws -> Connection
    ) async throws -> Connection {
        guard case .password(let fields) = kind.authorization else {
            current.waitingStyle = .browser
            return try await attempt()
        }
        current.waitingStyle = .inApp
        let help = (kind as? CredentialPromptHelp)?.credentialHelp
        var values = prefill
        var message: String?
        while true {
            await host.signIn.prepare(title: "\(kind.displayName) account", help: help, values: values, error: message)
            do {
                return try await attempt()
            } catch let error as CancellationError {
                throw error
            } catch {
                if Task.isCancelled { throw CancellationError() }
                guard let typed = await host.signIn.lastNonSecretValues() else { throw error }
                values = typed
                message = Self.describeSignIn(error, fields: fields)
            }
        }
    }

    /// "Apple ID or app-specific password was not accepted." for a rejected password, in the kind's own words;
    /// "iCal link was not accepted." for a kind whose only field is secret; the usual description otherwise.
    static func describeSignIn(_ error: Error, fields: [CredentialField]) -> String {
        guard case CalendarCore.SourceError.authExpired = error, let secret = fields.first(where: \.isSecret) else { return describe(error) }
        guard let name = fields.first(where: { !$0.isSecret && $0.key == "username" }) ?? fields.first(where: { !$0.isSecret }) else {
            return "\(secret.label) was not accepted."
        }
        return "\(name.label) or \(secret.label.prefix(1).lowercased() + secret.label.dropFirst()) was not accepted."
    }

    /// Order matters so an interruption never leaves a half-removed account that looks alive: the stored connection
    /// goes first, the source is then stopped through the reconciler, the calendar selections are forgotten, and
    /// secrets and sync state are deleted last, best effort.
    public func removeAccount(connectionID: ConnectionID) async {
        guard let connection = current.accounts.first(where: { $0.connectionID == connectionID }) else {
            invalidCommand("That account no longer exists.", "removeAccount: no account \(connectionID)")
            return
        }
        guard removing.insert(connectionID).inserted else { return }
        defer { removing.remove(connectionID) }
        let sourceID = statusKey(for: connection)
        do { try await connectionStore.remove(connectionID: connectionID) }
        catch { current.errorMessage = Self.describe(error); return }
        current.accounts.removeAll { $0.connectionID == connectionID }
        await reconcileAndApply()
        await updateSettings { $0.takeover.removeCalendars(forSourceID: sourceID) }
        await syncState.removeAll(for: connectionID)
        do { try await host.credentials.removeSecrets(for: connectionID) }
        catch { current.errorMessage = "The account was removed, but its saved sign-in could not be deleted: \(Self.describe(error))" }
    }

    public func reauthorize(connectionID: ConnectionID) async {
        guard let connection = current.accounts.first(where: { $0.connectionID == connectionID }) else {
            invalidCommand("That account no longer exists.", "reauthorizeAccount: no account \(connectionID)")
            return
        }
        guard let kind = host.registry.kind(id: connection.kindID) else {
            invalidCommand("Unknown account type.", "reauthorizeAccount: unknown account type \(connection.kindID)")
            return
        }
        guard !removing.contains(connectionID) else {
            current.errorMessage = "That account is being removed."
            return
        }
        current.isWorking = true
        current.errorMessage = nil
        defer { current.isWorking = false }
        do {
            let interaction = host.interaction, credentials = host.credentials
            let updated = try await signIn(with: kind, prefill: connection.config) {
                try await kind.reauthorize(connection, using: interaction, credentials: credentials)
            }
            try Task.checkCancellation()
            guard updated.connectionID == connectionID else {
                await discardSecretsIfUnowned(updated.connectionID)
                current.errorMessage = "Sign-in failed: signed in as a different account."
                return
            }
            guard !removing.contains(connectionID) else {
                current.errorMessage = "That account is being removed."
                return
            }
            // Persist first, so memory never runs ahead of the file.
            try await connectionStore.add(updated)
            if let index = current.accounts.firstIndex(where: { $0.connectionID == connectionID }) { current.accounts[index] = updated }
            let update = await reconciler.rebuild(
                updated, connections: current.accounts, systemCalendarEnabled: await settings().systemCalendarEnabled)
            await finish(update)
        } catch is CancellationError {
        } catch {
            current.errorMessage = Self.describe(error)
        }
    }

    /// Turning it off hides the calendars but keeps their stored selections, so turning it back on is lossless.
    public func setSystemCalendarEnabled(_ enabled: Bool) async {
        await updateSettings { $0.systemCalendarEnabled = enabled }
        if enabled { await host.requestSystemAccess?() }
        await reconcileAndApply()
    }

    /// The engine's own settings already hold the new value: requests access when it is on, then rebuilds the
    /// sources from `settings()` as they are now, so a later external change is never overwritten.
    func systemCalendarSettingChanged() async {
        if await settings().systemCalendarEnabled { await host.requestSystemAccess?() }
        await reconcileAndApply()
    }

    /// A command naming an account or account type that does not exist: shown to the user and diagnosed (ids only).
    private func invalidCommand(_ message: String, _ diagnostic: String) {
        current.errorMessage = message
        diagnose(EngineDiagnostic(level: .warning, category: "accounts", message: diagnostic))
    }

    public func stop() async {
        authTask?.cancel()
        await reconciler.stop()
    }

    private func reconcileAndApply() async {
        let enabled = await settings().systemCalendarEnabled
        let update = await reconciler.reconcile(connections: current.accounts, systemCalendarEnabled: enabled)
        await finish(update)
    }

    private func finish(_ update: SourceReconcilerUpdate) async {
        current.buildFailures = update.failures
        var ids: [ConnectionID: String] = [:]
        for account in current.accounts {
            if let id = await reconciler.sourceID(forConnection: account.connectionID) { ids[account.connectionID] = id }
        }
        sourceIDs = ids
        for (id, message) in update.failures {
            diagnose(EngineDiagnostic(level: .warning, category: "accounts", message: "source for \(id) failed: \(message)"))
        }
        await applySources(update.sources)
    }

    /// Drops stored selections of account-based sources that no stored account owns (an interrupted removal).
    /// Never runs on an unreadable file and never touches system calendar (`eventkit/...`) keys.
    private func sweepOrphanedSelections() async {
        let prefixes = availableKinds().map { "\($0.id)-" }
        let known = Set(current.accounts.map { statusKey(for: $0) } + current.accounts.map(\.sourceID))
        await updateSettings {
            $0.takeover.removeCalendars(whereSourceID: { id in
                prefixes.contains { id.hasPrefix($0) } && !known.contains(id)
            })
        }
    }

    private func availableKinds() -> [any ConnectorKind] { Self.availableKinds(in: host.registry) }

    private static func availableKinds(in registry: ConnectorRegistry) -> [any ConnectorKind] {
        let kinds = registry.kinds(for: .current).filter { if case .system = $0.authorization { false } else { true } }
        return kinds.filter { $0.id != "caldav" } + kinds.filter { $0.id == "caldav" }
    }

    private static func describe(_ error: Error) -> String {
        switch error {
        case CalendarCore.SourceError.authExpired: "Sign-in was not completed."
        case CalendarCore.SourceError.invalidResponse(let message): "Sign-in failed: \(message)."
        case let e as CalendarCore.SourceError: "Sign-in failed (\(e))."
        default: error.localizedDescription
        }
    }
}
