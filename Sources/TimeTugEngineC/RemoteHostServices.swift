import CalendarCore
import Foundation
import TimeTugCore
import TimeTugEngine
import TimeTugEngineProtocol

// The engine's host seams implemented as `service.*` requests answered through `tt_engine_reply`.
//
// Timeouts run on the engine clock. HTTP requests time out after 60 seconds (the spec's rule) and so do the other
// requests no person takes part in (credentials, closing a redirect, preparing a form, text generation), so a host
// that never answers cannot stall a refresh forever. The steps a person takes part in (the browser sign-in and the
// credential form) have no timeout; `account.cancel` and destroy end them.

/// Seconds the engine waits for a host answer that involves no person.
let hostServiceTimeout: TimeInterval = 60

private func wire(_ value: some Encodable) -> JSONValue { JSONValue(encoding: value) }

struct RemoteCredentialStore: CredentialStore {
    let table: RequestTable
    /// The host's own spelling of each connection's latest revision. The engine holds revisions as `UUID`s, whose
    /// `uuidString` is upper case; a host that wrote the revision in lower case (.NET's `Guid.ToString()`) must get
    /// back exactly the string it sent, or its compare-and-set never matches.
    private let spellings = RevisionSpellings()

    init(table: RequestTable) { self.table = table }

    private func key(_ id: ConnectionID) -> JSONValue { wire(CredentialsKeyRequest(connectionID: id)) }

    func secrets(for connectionID: ConnectionID) async throws -> [String: String]? {
        let reply: CredentialsGetReply = try await table.call(
            "service.credentials.get", payload: key(connectionID), timeout: hostServiceTimeout)
        return reply.secrets
    }

    func setSecrets(_ secrets: [String: String], for connectionID: ConnectionID) async throws {
        _ = try await table.call(
            "service.credentials.set", payload: wire(CredentialsSetRequest(connectionID: connectionID, secrets: secrets)),
            timeout: hostServiceTimeout, as: IgnoredResult.self)
    }

    func removeSecrets(for connectionID: ConnectionID) async throws {
        _ = try await table.call(
            "service.credentials.remove", payload: key(connectionID), timeout: hostServiceTimeout, as: IgnoredResult.self)
    }

    func credentialSnapshot(for connectionID: ConnectionID) async throws -> CredentialSnapshot? {
        let reply: CredentialsSnapshotReply = try await table.call(
            "service.credentials.snapshot", payload: key(connectionID), timeout: hostServiceTimeout)
        guard let snapshot = reply.snapshot else { return nil }
        guard let revision = UUID(uuidString: snapshot.revision) else {
            throw ServiceError(kind: .failed, message: "the credential revision is not a UUID")
        }
        spellings.record(snapshot.revision, as: revision, for: connectionID)
        return CredentialSnapshot(secrets: snapshot.secrets, revision: revision)
    }

    func updateRefreshToken(_ token: String, for connectionID: ConnectionID, expectedRevision: UUID) async throws -> Bool {
        let request = CredentialsUpdateRefreshTokenRequest(
            connectionID: connectionID, token: token,
            expectedRevision: spellings.spelling(of: expectedRevision, for: connectionID))
        let reply: CredentialsUpdateRefreshTokenReply = try await table.call(
            "service.credentials.updateRefreshToken", payload: wire(request), timeout: hostServiceTimeout)
        return reply.updated
    }
}

/// The last revision string the host gave per connection (one entry per connection, so it stays small).
final class RevisionSpellings: @unchecked Sendable {
    private let lock = NSLock()
    private var latest: [ConnectionID: (revision: UUID, text: String)] = [:]

    func record(_ text: String, as revision: UUID, for connectionID: ConnectionID) {
        lock.withLock { latest[connectionID] = (revision, text) }
    }

    /// The host's spelling when `revision` is the one it gave last for the connection, otherwise `uuidString`.
    func spelling(of revision: UUID, for connectionID: ConnectionID) -> String {
        lock.withLock {
            guard let entry = latest[connectionID], entry.revision == revision else { return revision.uuidString }
            return entry.text
        }
    }
}

/// HTTP through the host. Any status is a result; a host transport failure, any other error reply and the 60 s
/// timeout are `SourceError.network`; a cancelled reply is a cancellation.
struct RemoteHTTPTransport: HTTPTransport {
    static let timeout: TimeInterval = 60
    let table: RequestTable

    func send(_ request: HTTPRequest) async throws -> HTTPResponse {
        let outgoing = HTTPSendRequest(
            method: request.method, url: request.url.absoluteString, headers: request.headers,
            body: request.body.map { $0.base64EncodedString() })
        let reply: HTTPSendReply
        do {
            reply = try await table.call("service.http.send", payload: wire(outgoing), timeout: Self.timeout)
        } catch let error as ServiceError {
            throw SourceError.network(error.description)
        }
        var body = Data()
        if let encoded = reply.body {
            guard let decoded = Data(base64Encoded: encoded) else {
                throw SourceError.invalidResponse("the host's HTTP body is not base64")
            }
            body = decoded
        }
        return HTTPResponse(status: reply.status, headers: reply.headers ?? [:], body: body)
    }
}

struct RemoteAuthorizationInteraction: AuthorizationInteraction {
    let table: RequestTable

    func beginOAuthRedirect() async throws -> any OAuthRedirectSession {
        let reply: AuthBeginRedirectReply = try await table.call("service.auth.beginRedirect", timeout: nil)
        guard let uri = URL(string: reply.redirectURI) else {
            throw ServiceError(kind: .failed, message: "the redirect URI is not a URL")
        }
        return RemoteRedirectSession(table: table, sessionID: reply.sessionID, redirectURI: uri)
    }

    func promptCredentials(_ fields: [CredentialField]) async throws -> [String: String] {
        let request = AuthPromptCredentialsRequest(
            fields: fields.map { WireCredentialField(key: $0.key, label: $0.label, isSecret: $0.isSecret) })
        let reply: AuthPromptCredentialsReply = try await table.call(
            "service.auth.promptCredentials", payload: wire(request), timeout: nil)
        return reply.values
    }
}

struct RemoteRedirectSession: OAuthRedirectSession {
    let table: RequestTable
    let sessionID: String
    let redirectURI: URL

    func authorize(at authorizationURL: URL) async throws -> URL {
        let request = AuthAuthorizeRequest(sessionID: sessionID, url: authorizationURL.absoluteString)
        let reply: AuthAuthorizeReply = try await table.call(
            "service.auth.authorize", payload: wire(request), timeout: nil)
        guard let url = URL(string: reply.url) else {
            throw ServiceError(kind: .failed, message: "the redirect is not a URL")
        }
        return url
    }

    /// Best effort: the sign-in already has its answer, so a failure to close is not reported.
    func close() async {
        _ = try? await table.call(
            "service.auth.closeRedirect", payload: wire(AuthCloseRedirectRequest(sessionID: sessionID)),
            timeout: hostServiceTimeout, as: IgnoredResult.self)
    }
}

struct RemoteSignInPrompting: SignInPrompting {
    let table: RequestTable

    func prepare(title: String, help: CredentialHelp?, values: [String: String], error: String?) async {
        let request = SignInPrepareRequest(
            title: title,
            help: help.map { WireCredentialHelp(text: $0.text, linkTitle: $0.linkTitle, url: $0.url?.absoluteString) },
            values: values, error: error)
        _ = try? await table.call(
            "service.signIn.prepare", payload: wire(request), timeout: hostServiceTimeout, as: IgnoredResult.self)
    }

    func lastNonSecretValues() async -> [String: String]? {
        let reply = try? await table.call(
            "service.signIn.lastNonSecretValues", timeout: hostServiceTimeout, as: SignInLastValuesReply.self)
        return reply?.values
    }
}

/// The host's text model (present only when the config asks for text generation).
struct RemoteTextGenerator: TextGenerator {
    let table: RequestTable

    var availability: TextGeneratorAvailability {
        get async {
            let reply: TextAvailabilityReply
            do {
                reply = try await table.call("service.text.availability", timeout: hostServiceTimeout)
            } catch {
                return .temporarilyUnavailable(reason: "the host did not answer")
            }
            switch reply.kind {
            case "available":
                guard let id = reply.engineID, let name = reply.engineName else {
                    return .unavailable(reason: "the host named no engine")
                }
                return .available(EngineInfo(id: id, displayName: name, isOnDevice: reply.isOnDevice ?? false))
            case "temporarilyUnavailable": return .temporarilyUnavailable(reason: reply.reason ?? "")
            default: return .unavailable(reason: reply.reason ?? "")
            }
        }
    }

    func generate(instructions: String, prompt: String) async throws -> String {
        let reply: TextGenerateReply = try await table.call(
            "service.text.generate", payload: wire(TextGenerateRequest(instructions: instructions, prompt: prompt)),
            timeout: hostServiceTimeout)
        return reply.text
    }
}

/// The host's own account service (Windows' Web Account Manager) as the library's `ExternalTokenSource`, over
/// `service.token.*`. Present only when the config sets `externalTokens`.
///
/// Silent requests time out after `hostServiceTimeout` on the engine clock. A sign-in has no timeout: it ends with
/// the host's answer or with a cancellation, which tells the host to close its dialog (`service.token.cancelSignIn`,
/// best effort). Nothing a diagnostic carries comes from the host except a `reason` that is one of the documented
/// codes; tokens, account ids, emails and the host's error text never reach one or a thrown error.
struct RemoteExternalTokenSource: ExternalTokenSource {
    /// The `reason` codes a host may send, as documented in `docs/engine-protocol.md`.
    static let documentedReasons: Set<String> = [
        "consent", "signedOut", "mfa", "policy", "accountMissing", "adminConsentRequired", "brokerUnavailable", "other",
    ]
    private static let incomplete = CalendarCore.SourceError.invalidResponse("the host's token answer was incomplete")
    private static let unavailable = CalendarCore.SourceError.invalidResponse("Windows sign-in is not available")

    let table: RequestTable
    let diagnostics: EngineDiagnosticSink

    init(table: RequestTable, diagnostics: @escaping EngineDiagnosticSink = { _ in }) {
        self.table = table
        self.diagnostics = diagnostics
    }

    func accessToken(for account: ExternalAccount, scopes: [String], forceRefresh: Bool) async throws -> ExternalAccessToken {
        let request = TokenGetRequest(
            connectionID: account.connectionID ?? "", accountID: account.id, email: account.email, scopes: scopes,
            forceRefresh: forceRefresh)
        let reply: TokenGetReply
        do {
            reply = try await table.call("service.token.get", payload: wire(request), timeout: hostServiceTimeout)
        } catch let error as ServiceError {
            throw Self.sourceError(for: error)
        }
        noteReason(reply.reason)
        switch reply.status {
        case "ok": return try Self.token(reply.accessToken, reply.expiresAt)
        case "interactionRequired": throw CalendarCore.SourceError.authExpired
        case "unavailable": throw Self.unavailable
        default: throw Self.incomplete
        }
    }

    func signIn(_ request: ExternalSignInRequest) async throws -> ExternalSignInResult {
        let signInID = UUID().uuidString
        let wireRequest = TokenSignInRequest(
            signInID: signInID, scopes: request.scopes, accountID: request.account?.id,
            email: request.email ?? request.account?.email)
        let table = table
        let answered = LockedFlag()
        let reply: TokenSignInReply
        do {
            reply = try await withTaskCancellationHandler {
                defer { answered.set() }
                return try await table.call("service.token.signIn", payload: wire(wireRequest), timeout: nil)
            } onCancel: {
                // Best effort, and only while the dialog may still be open: an answer that already arrived needs
                // no cancel. The task is unstructured on purpose: the cancelled sign-in's own task is done.
                guard !answered.isSet else { return }
                Task {
                    _ = try? await table.call(
                        "service.token.cancelSignIn", payload: wire(TokenCancelSignInRequest(signInID: signInID)),
                        timeout: hostServiceTimeout, as: IgnoredResult.self)
                }
            }
        } catch let error as ServiceError {
            throw Self.sourceError(for: error)
        }
        noteReason(reply.reason)
        switch reply.status {
        case "ok":
            guard let accountID = reply.accountID, !accountID.isEmpty else { throw Self.incomplete }
            return ExternalSignInResult(
                account: ExternalAccount(id: accountID, email: request.email ?? request.account?.email,
                                         connectionID: request.account?.connectionID),
                token: try Self.token(reply.accessToken, reply.expiresAt))
        case "blocked": throw CalendarCore.SourceError.invalidResponse("admin approval is required")
        case "unavailable": throw Self.unavailable
        default: throw Self.incomplete
        }
    }

    private static func token(_ accessToken: String?, _ expiresAt: String?) throws -> ExternalAccessToken {
        guard let accessToken, !accessToken.isEmpty, let expiresAt, let date = WireDates.date(expiresAt) else {
            throw incomplete
        }
        return ExternalAccessToken(accessToken: accessToken, expiresAt: date)
    }

    /// The host's error text is not used: it could carry account data.
    private static func sourceError(for error: ServiceError) -> CalendarCore.SourceError {
        switch error.kind {
        case .network: .network("Windows sign-in could not reach the network")
        case .notFound, .denied: unavailable
        case .failed, .cancelled: .invalidResponse("the Windows account service failed")
        }
    }

    private func noteReason(_ reason: String?) {
        guard let reason, Self.documentedReasons.contains(reason) else { return }
        diagnostics(EngineDiagnostic(level: .info, category: "tokens", message: "the host reported reason \(reason)"))
    }
}

/// A flag set once from one thread and read from another.
private final class LockedFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    var isSet: Bool { lock.withLock { value } }
    func set() { lock.withLock { value = true } }
}
