import CalendarCore
import Foundation
import TimeTugCore
@testable import TimeTugEngine

struct FakeSource: TimeTugCore.CalendarSource {
    let id: String
    var displayName: String { id }
    func calendars() async throws -> [CalendarInfo] { [] }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] { [] }
    func changes() -> AsyncStream<SourceChange> { AsyncStream { $0.finish() } }
}

/// A reconciler fake: builds a `FakeSource` per connection (and "eventkit" when the system calendar is on).
actor FakeReconciler: SourceReconciling {
    private(set) var builds = 0
    private(set) var stopped = false
    let log: LockedBox<[String]>
    let accountsFile: URL?
    /// Whether each reconcile saw the stored account file still listing "1" (for the removal-order test).
    private(set) var storedAtReconcile: [Bool] = []
    init(log: LockedBox<[String]> = LockedBox([]), accountsFile: URL? = nil) {
        self.log = log
        self.accountsFile = accountsFile
    }

    private func update(_ connections: [Connection], _ systemOn: Bool) -> SourceReconcilerUpdate {
        var sources: [any TimeTugCore.CalendarSource] = []
        if systemOn { sources.append(FakeSource(id: "eventkit")) }
        sources += connections.map { FakeSource(id: $0.sourceID) }
        return SourceReconcilerUpdate(sources: sources, failures: [:])
    }

    func reconcile(connections: [Connection], systemCalendarEnabled: Bool) async -> SourceReconcilerUpdate {
        log.mutate { $0.append("reconcile") }
        builds += connections.count
        if let accountsFile, let data = try? Data(contentsOf: accountsFile),
           let list = try? JSONDecoder().decode([Connection].self, from: data) {
            storedAtReconcile.append(list.contains { $0.connectionID == "1" })
        }
        return update(connections, systemCalendarEnabled)
    }

    func rebuild(_ connection: Connection, connections: [Connection], systemCalendarEnabled: Bool) async -> SourceReconcilerUpdate {
        builds += 1
        return update(connections, systemCalendarEnabled)
    }

    func sourceID(forConnection id: ConnectionID) async -> String? { nil }
    func stop() async { stopped = true }
}

/// A kind with configurable authorization method and outcome.
final class FakeConnectorKind: ConnectorKind, @unchecked Sendable {
    enum Outcome: Sendable {
        case succeed
        /// Stores secrets for `connectionID`, returns the connection, and cancels the signing-in task as it returns.
        case succeedThenCancel(connectionID: String)
        /// Rejects the password until it equals `good`.
        case password(good: String)
    }
    let id: String
    let displayName: String
    let supportedPlatforms: Platform = [.macOS, .iOS, .linux, .windows]
    let authorization: AuthorizationMethod
    var outcome: Outcome
    var nextEmail = "a@x.test"
    var nextConnectionID = "1"
    var reauthorizeError: Error?
    var reauthorizeSuspends = false
    var reauthorizeResult: Connection?
    /// When set, reauthorize cancels the signing-in task and returns the connection renamed to this.
    var reauthorizeCancelsAndRenames: String?
    /// Builds the source for a connection; unset, `makeSource` throws.
    var sourceFactory: (@Sendable (Connection) throws -> any CalendarCore.CalendarSource)?
    private let lock = NSLock()
    private var _reauthorized = 0, _attempts = 0
    var reauthorized: Int { lock.withLock { _reauthorized } }
    var attempts: Int { lock.withLock { _attempts } }

    init(id: String = "google", displayName: String = "Google", authorization: AuthorizationMethod = .oauth, outcome: Outcome = .succeed) {
        self.id = id
        self.displayName = displayName
        self.authorization = authorization
        self.outcome = outcome
    }

    func authorize(using interaction: any AuthorizationInteraction, credentials: any CredentialStore) async throws -> Connection {
        lock.withLock { _attempts += 1 }
        switch outcome {
        case .succeed:
            let c = Connection(kindID: id, connectionID: nextConnectionID, displayName: nextEmail, config: ["email": nextEmail])
            try await credentials.setSecrets(["refresh_token": "r-\(nextConnectionID)"], for: c.connectionID)
            return c
        case .succeedThenCancel(let connectionID):
            try await credentials.setSecrets(["refresh_token": "r"], for: connectionID)
            withUnsafeCurrentTask { $0?.cancel() }
            return Connection(kindID: id, connectionID: connectionID, displayName: "c@x.test")
        case .password(let good):
            guard case .password(let fields) = authorization else { throw CalendarCore.SourceError.invalidResponse("not a password kind") }
            let values = try await interaction.promptCredentials(fields)
            guard values["password"] == good else { throw CalendarCore.SourceError.authExpired }
            let c = Connection(kindID: id, connectionID: "p\(attempts)", displayName: values["username"] ?? "",
                               config: ["username": values["username"] ?? ""])
            try await credentials.setSecrets(["password": values["password"] ?? ""], for: c.connectionID)
            return c
        }
    }

    func reauthorize(_ connection: Connection, using interaction: any AuthorizationInteraction, credentials: any CredentialStore) async throws -> Connection {
        if reauthorizeSuspends { try await Task.sleep(for: .seconds(30)) }
        if let reauthorizeError { throw reauthorizeError }
        lock.withLock { _reauthorized += 1 }
        if let name = reauthorizeCancelsAndRenames {
            withUnsafeCurrentTask { $0?.cancel() }
            var renamed = connection
            renamed.displayName = name
            return renamed
        }
        return reauthorizeResult ?? connection
    }

    func makeSource(for connection: Connection, credentials: any CredentialStore, syncState: any SyncStateStore) throws -> any CalendarCore.CalendarSource {
        if let sourceFactory { return try sourceFactory(connection) }
        throw CalendarCore.SourceError.invalidResponse("unused")
    }
}

/// Plays the host's sign-in form: records what it was armed with and answers prompts from a queue of password attempts.
final class FakeSignIn: SignInPrompting, AuthorizationInteraction, @unchecked Sendable {
    struct Armed: Equatable, Sendable { var title: String; var help: CredentialHelp?; var values: [String: String]; var error: String? }
    private let lock = NSLock()
    private var armed: [Armed] = []
    private var passwords: [String]
    private var typedValues: [String: String]
    init(passwords: [String] = [], typed: [String: String] = ["username": "me"]) {
        self.passwords = passwords
        self.typedValues = typed
    }
    var prepared: [Armed] { lock.withLock { armed } }
    func prepare(title: String, help: CredentialHelp?, values: [String: String], error: String?) async {
        lock.withLock { armed.append(Armed(title: title, help: help, values: values, error: error)) }
    }
    func lastNonSecretValues() async -> [String: String]? { lock.withLock { typedValues } }
    func beginOAuthRedirect() async throws -> any OAuthRedirectSession { throw CalendarCore.SourceError.invalidResponse("unused") }
    func promptCredentials(_ fields: [CredentialField]) async throws -> [String: String] {
        let next: String? = lock.withLock { passwords.isEmpty ? nil : passwords.removeFirst() }
        guard let next else { throw CancellationError() }
        return typedValues.merging(["password": next]) { $1 }
    }
}

struct FailingRemovalCredentials: CredentialStore {
    struct Failure: Error {}
    let inner: InMemoryCredentialStore
    func secrets(for id: ConnectionID) async throws -> [String: String]? { try await inner.secrets(for: id) }
    func setSecrets(_ s: [String: String], for id: ConnectionID) async throws { try await inner.setSecrets(s, for: id) }
    func removeSecrets(for id: ConnectionID) async throws { throw Failure() }
    func credentialSnapshot(for id: ConnectionID) async throws -> CredentialSnapshot? { try await inner.credentialSnapshot(for: id) }
    func updateRefreshToken(_ t: String, for id: ConnectionID, expectedRevision: UUID) async throws -> Bool {
        try await inner.updateRefreshToken(t, for: id, expectedRevision: expectedRevision)
    }
}

/// Wraps a credential store and logs removals.
struct LoggingCredentials: CredentialStore {
    let inner: InMemoryCredentialStore
    let log: LockedBox<[String]>
    func secrets(for id: ConnectionID) async throws -> [String: String]? { try await inner.secrets(for: id) }
    func setSecrets(_ s: [String: String], for id: ConnectionID) async throws { try await inner.setSecrets(s, for: id) }
    func removeSecrets(for id: ConnectionID) async throws {
        log.mutate { $0.append("secrets") }
        try await inner.removeSecrets(for: id)
    }
    func credentialSnapshot(for id: ConnectionID) async throws -> CredentialSnapshot? { try await inner.credentialSnapshot(for: id) }
    func updateRefreshToken(_ t: String, for id: ConnectionID, expectedRevision: UUID) async throws -> Bool {
        try await inner.updateRefreshToken(t, for: id, expectedRevision: expectedRevision)
    }
}

/// Suspends callers of `wait()` until `open()`; `isWaiting` says a caller has arrived.
actor Gate {
    private var continuation: CheckedContinuation<Void, Never>?
    private var opened = false
    var isWaiting: Bool { continuation != nil }
    func wait() async {
        if opened { return }
        await withCheckedContinuation { continuation = $0 }
    }
    func open() {
        opened = true
        continuation?.resume()
        continuation = nil
    }
}

struct LoggingSyncState: SyncStateStore {
    let inner: InMemorySyncStateStore
    let log: LockedBox<[String]>
    var gate: Gate?
    func token(for id: ConnectionID, scope: String) async -> String? { await inner.token(for: id, scope: scope) }
    func setToken(_ token: String?, for id: ConnectionID, scope: String) async { await inner.setToken(token, for: id, scope: scope) }
    func removeAll(for id: ConnectionID) async {
        log.mutate { $0.append("sync") }
        await gate?.wait()
        await inner.removeAll(for: id)
    }
}

/// An `AccountManager` wired to temp storage, in-memory stores and a recording `applySources`.
struct AccountHarness {
    let directory: URL
    var accountsFile: URL { directory.appendingPathComponent("accounts.json") }
    let manager: AccountManager
    let reconciler: FakeReconciler
    let credentials: InMemoryCredentialStore
    let syncState: InMemorySyncStateStore
    let settings: InMemorySettingsStorage
    let signIn: FakeSignIn
    let applied: LockedBox<[[String]]>
    let log: LockedBox<[String]>

    init(
        kinds: [FakeConnectorKind], passwords: [String] = [], failingRemoval: Bool = false,
        initial: EngineSettings = EngineSettings(), directory: URL? = nil, syncGate: Gate? = nil
    ) throws {
        let dir = try directory ?? TempDir.make()
        self.directory = dir
        log = LockedBox([])
        credentials = InMemoryCredentialStore()
        syncState = InMemorySyncStateStore()
        settings = InMemorySettingsStorage(initial)
        signIn = FakeSignIn(passwords: passwords)
        applied = LockedBox([])
        reconciler = FakeReconciler(log: log, accountsFile: dir.appendingPathComponent("accounts.json"))
        var registry = ConnectorRegistry()
        for kind in kinds { registry.register(kind) }
        let store: any CredentialStore = failingRemoval
            ? FailingRemovalCredentials(inner: credentials) : LoggingCredentials(inner: credentials, log: log)
        let host = EngineHost(
            dataDirectory: dir, settings: settings, credentials: store, interaction: signIn, signIn: signIn,
            registry: registry, requestSystemAccess: { })
        let settings = self.settings, applied = self.applied, log = self.log
        manager = AccountManager(
            host: host, reconciler: reconciler, syncState: LoggingSyncState(inner: syncState, log: log, gate: syncGate),
            settings: { settings.load() },
            applySources: { sources in applied.mutate { $0.append(sources.map(\.id)) } },
            updateSettings: { change in
                var s = settings.load()
                change(&s)
                settings.save(s)
                log.mutate { $0.append("selections") }
            })
    }
}
