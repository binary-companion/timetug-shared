import CalendarCore
import Foundation
import TimeTugCore

/// The engine's view of the host's sign-in form (the Mac `CredentialPrompter`; Windows implements the same).
/// `prepare` arms the form for the next `promptCredentials` call; `lastNonSecretValues` is what the user last
/// typed, without secrets, or nil when the sign-in failed before a form was submitted.
public protocol SignInPrompting: Sendable {
    func prepare(title: String, help: CredentialHelp?, values: [String: String], error: String?) async
    func lastNonSecretValues() async -> [String: String]?
}

/// Everything the host supplies to the engine.
public struct EngineHost: Sendable {
    public var dataDirectory: URL
    public var settings: any SettingsStorage
    public var credentials: any CredentialStore
    public var interaction: any AuthorizationInteraction
    public var signIn: any SignInPrompting
    public var registry: ConnectorRegistry
    /// Kind ids supported in code but not registered in this build (for example Google without an OAuth client).
    public var unconfiguredKindIDs: [String]
    public var buildSystemSource: (@Sendable () -> any TimeTugCore.CalendarSource)?
    public var requestSystemAccess: (@Sendable () async -> Void)?
    public var textGenerator: (any TextGenerator)?
    public var diagnostics: EngineDiagnosticSink
    public var clock: any EngineClock
    public var calendar: @Sendable () -> Calendar

    public init(
        dataDirectory: URL, settings: any SettingsStorage, credentials: any CredentialStore,
        interaction: any AuthorizationInteraction, signIn: any SignInPrompting, registry: ConnectorRegistry,
        unconfiguredKindIDs: [String] = [],
        buildSystemSource: (@Sendable () -> any TimeTugCore.CalendarSource)? = nil,
        requestSystemAccess: (@Sendable () async -> Void)? = nil,
        textGenerator: (any TextGenerator)? = nil,
        diagnostics: @escaping EngineDiagnosticSink = { _ in },
        clock: any EngineClock = SystemClock(),
        calendar: @escaping @Sendable () -> Calendar = { .current }
    ) {
        self.dataDirectory = dataDirectory
        self.settings = settings
        self.credentials = credentials
        self.interaction = interaction
        self.signIn = signIn
        self.registry = registry
        self.unconfiguredKindIDs = unconfiguredKindIDs
        self.buildSystemSource = buildSystemSource
        self.requestSystemAccess = requestSystemAccess
        self.textGenerator = textGenerator
        self.diagnostics = diagnostics
        self.clock = clock
        self.calendar = calendar
    }
}
