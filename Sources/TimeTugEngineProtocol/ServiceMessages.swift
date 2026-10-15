import Foundation

/// The engine-to-host service requests (`service.*`) and their replies. A request is an `Envelope` whose `id` is
/// the `uint64` the host passes back to `tt_engine_reply`; a reply is `{"v":1,"result":...}` or
/// `{"v":1,"error":{"kind","message"}}`. Binary bodies are base64.
public enum ServiceMessages {
    public static let types: [String] = [
        "service.credentials.get", "service.credentials.set", "service.credentials.remove",
        "service.credentials.snapshot", "service.credentials.updateRefreshToken",
        "service.http.send",
        "service.auth.beginRedirect", "service.auth.authorize", "service.auth.closeRedirect",
        "service.auth.promptCredentials", "service.signIn.prepare", "service.signIn.lastNonSecretValues",
        "service.text.availability", "service.text.generate",
    ]
}

public enum ReplyErrorKind: String, Codable, Equatable, Sendable {
    case network, cancelled, notFound, denied, failed
}

public struct WireReplyError: Codable, Equatable, Sendable {
    public var kind: String
    public var message: String?
    public init(kind: ReplyErrorKind, message: String? = nil) { self.kind = kind.rawValue; self.message = message }
}

/// A parsed reply: a result value or an error.
public enum WireReply: Equatable, Sendable {
    case result(JSONValue)
    case error(kind: ReplyErrorKind, message: String)

    /// Never throws: anything that is not a well-formed v1 reply becomes a `failed` error, so a request is always
    /// resolved. An unknown error kind is `failed`.
    public static func parse(_ json: String) -> WireReply {
        guard let value = try? JSONDecoder().decode(JSONValue.self, from: Data(json.utf8)), case .object = value else {
            return .error(kind: .failed, message: "reply is not a JSON object")
        }
        guard case .number(let v)? = value["v"], Int(exactly: v).map(ProtocolVersion.supported.contains) == true else {
            return .error(kind: .failed, message: "unsupported reply version")
        }
        if let present = value["error"], !{ if case .object = present { return true } else { return false } }() {
            return .error(kind: .failed, message: "reply error is not an object")
        }
        if case .object(let error)? = value["error"] {
            var kind = ReplyErrorKind.failed
            if case .string(let k)? = error["kind"], let parsed = ReplyErrorKind(rawValue: k) { kind = parsed }
            var message = ""
            if case .string(let m)? = error["message"] { message = m }
            return .error(kind: kind, message: message)
        }
        return .result(value["result"] ?? .null)
    }
}

// MARK: - Credentials

public struct CredentialsKeyRequest: Codable, Equatable, Sendable {
    public var connectionID: String
    public init(connectionID: String) { self.connectionID = connectionID }
}

public struct CredentialsSetRequest: Codable, Equatable, Sendable {
    public var connectionID: String
    public var secrets: [String: String]
    public init(connectionID: String, secrets: [String: String]) {
        self.connectionID = connectionID
        self.secrets = secrets
    }
}

/// Reply to `get`: `secrets` absent when the connection has none.
public struct CredentialsGetReply: Codable, Equatable, Sendable {
    public var secrets: [String: String]?
}

public struct CredentialsSnapshotReply: Codable, Equatable, Sendable {
    public struct Snapshot: Codable, Equatable, Sendable {
        public var secrets: [String: String]
        /// An opaque revision string (a UUID); the engine passes it back unchanged.
        public var revision: String
    }
    public var snapshot: Snapshot?
}

public struct CredentialsUpdateRefreshTokenRequest: Codable, Equatable, Sendable {
    public var connectionID: String
    public var token: String
    public var expectedRevision: String
    public init(connectionID: String, token: String, expectedRevision: String) {
        self.connectionID = connectionID
        self.token = token
        self.expectedRevision = expectedRevision
    }
}

public struct CredentialsUpdateRefreshTokenReply: Codable, Equatable, Sendable {
    /// False when the stored revision no longer matched (nothing was written).
    public var updated: Bool
}

// MARK: - HTTP

public struct HTTPSendRequest: Codable, Equatable, Sendable {
    public var method: String
    public var url: String
    public var headers: [String: String]
    /// Base64; absent for no body.
    public var body: String?
    public init(method: String, url: String, headers: [String: String], body: String?) {
        self.method = method
        self.url = url
        self.headers = headers
        self.body = body
    }
}

public struct HTTPSendReply: Codable, Equatable, Sendable {
    public var status: Int
    public var headers: [String: String]?
    /// Base64; absent for an empty body.
    public var body: String?
}

// MARK: - Sign-in

public struct AuthBeginRedirectReply: Codable, Equatable, Sendable {
    public var sessionID: String
    public var redirectURI: String
}

public struct AuthAuthorizeRequest: Codable, Equatable, Sendable {
    public var sessionID: String
    public var url: String
    public init(sessionID: String, url: String) { self.sessionID = sessionID; self.url = url }
}

public struct AuthAuthorizeReply: Codable, Equatable, Sendable {
    public var url: String
}

public struct AuthCloseRedirectRequest: Codable, Equatable, Sendable {
    public var sessionID: String
    public init(sessionID: String) { self.sessionID = sessionID }
}

public struct WireCredentialField: Codable, Equatable, Sendable {
    public var key: String
    public var label: String
    public var isSecret: Bool
    public init(key: String, label: String, isSecret: Bool) { self.key = key; self.label = label; self.isSecret = isSecret }
}

public struct AuthPromptCredentialsRequest: Codable, Equatable, Sendable {
    public var fields: [WireCredentialField]
    public init(fields: [WireCredentialField]) { self.fields = fields }
}

public struct AuthPromptCredentialsReply: Codable, Equatable, Sendable {
    public var values: [String: String]
}

public struct WireCredentialHelp: Codable, Equatable, Sendable {
    public var text: String
    public var linkTitle: String?
    public var url: String?
    public init(text: String, linkTitle: String? = nil, url: String? = nil) {
        self.text = text
        self.linkTitle = linkTitle
        self.url = url
    }
}

public struct SignInPrepareRequest: Codable, Equatable, Sendable {
    public var title: String
    public var help: WireCredentialHelp?
    public var values: [String: String]
    public var error: String?
    public init(title: String, help: WireCredentialHelp?, values: [String: String], error: String?) {
        self.title = title
        self.help = help
        self.values = values
        self.error = error
    }
}

public struct SignInLastValuesReply: Codable, Equatable, Sendable {
    /// Absent when the sign-in failed before a form was submitted.
    public var values: [String: String]?
}

// MARK: - Text generation

public struct TextAvailabilityReply: Codable, Equatable, Sendable {
    /// `available`, `unavailable` or `temporarilyUnavailable`.
    public var kind: String
    public var reason: String?
    public var engineID: String?
    public var engineName: String?
    public var isOnDevice: Bool?
}

public struct TextGenerateRequest: Codable, Equatable, Sendable {
    public var instructions: String
    public var prompt: String
    public init(instructions: String, prompt: String) { self.instructions = instructions; self.prompt = prompt }
}

public struct TextGenerateReply: Codable, Equatable, Sendable {
    public var text: String
}
