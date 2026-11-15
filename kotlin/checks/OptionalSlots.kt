// Compiled with the generated bindings by kotlin/compile-check.sh. It builds an EngineConfig and an EngineHostInterfaces from
// the required values only: every optional slot and setting has a default, so adding one later (docs/bindings.md, "Foreign
// traits and the rules for later changes") does not break a host written like this. Never run; the bodies are stubs.
package com.binarycompanion.timetug.engine.checks

import com.binarycompanion.timetug.engine.*

private class Credentials : CredentialStore {
    override suspend fun secrets(connectionId: String): Map<String, String>? = null
    override suspend fun setSecrets(secrets: Map<String, String>, connectionId: String) {}
    override suspend fun removeSecrets(connectionId: String) {}
    override suspend fun credentialSnapshot(connectionId: String): CredentialSnapshot? = null
    override suspend fun updateRefreshToken(token: String, connectionId: String, expectedRevision: java.util.UUID): Boolean = false
}

private class Authorization : AuthorizationInteraction {
    override suspend fun beginOauthRedirect(): OAuthRedirectHandle = throw NotImplementedError()
    override suspend fun promptCredentials(fields: List<CredentialField>): Map<String, String> = emptyMap()
}

private class SignIn : SignInPrompting {
    override suspend fun prepare(title: String, help: CredentialHelp?, values: Map<String, String>, error: String?) {}
    override suspend fun lastNonSecretValues(): Map<String, String>? = null
}

private class Settings : SettingsStorage {
    override fun load(): EngineSettings = throw NotImplementedError()
    override fun save(settings: EngineSettings) {}
}

private class Http : HttpTransport {
    override suspend fun send(request: HttpRequest): HttpResponse = throw NotImplementedError()
}

private class Observer : EngineObserver {
    override fun stateChanged(summary: EngineStateSummary) {}
    override fun event(event: EngineEvent) {}
}

/** Required values only: no Google or Microsoft client, no text generator, token source, system calendar or sink. */
fun engineWithoutOptionalSlots(): Engine {
    val config = EngineConfig(dataDirectory = "/data", platform = Platform.ANDROID, timeZone = "Europe/Berlin")
    val host = EngineHostInterfaces(
        credentials = Credentials(),
        authorization = Authorization(),
        signIn = SignIn(),
        settings = Settings(),
        http = Http(),
    )
    return Engine(config, host, Observer())
}

/** A sink implements both methods: foreign traits have no default methods. */
class LinesOnlySink : DiagnosticSink {
    override fun log(diagnostic: EngineDiagnostic) {}
    override fun record(event: DiagnosticEvent) {}
}
