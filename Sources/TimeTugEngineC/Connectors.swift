import CalDAVCalendar
import CalendarCore
import Foundation
import GoogleCalendar
import ICalSubscription
import MicrosoftCalendar
import TimeTugEngine
import TimeTugEngineProtocol

/// The connector kinds an engine behind the C interface offers: the library's network kinds, as the Mac app
/// registers them (`AppConnectors`), minus EventKit. Every kind sends HTTP through the host and reads time and sleeps
/// through the engine clock, so a `testing` engine's polls and token expiry follow `test.advanceClock` too.
enum ConnectorSetup {
    struct Result {
        var registry: ConnectorRegistry
        /// OAuth kinds this host build has no client for; listed as unconfigured in `availableAccountKinds`.
        var unconfiguredKindIDs: [String]
    }

    static func make(
        config: WireConfig, transport: any HTTPTransport, clock: any EngineClock, defaultZone: TimeZone
    ) -> Result {
        let now: @Sendable () -> Date = { clock.now() }
        let sleep: Sleeper = { duration in
            let (seconds, attoseconds) = duration.components
            let interval = Double(seconds) + Double(attoseconds) / 1e18
            try await clock.sleep(until: clock.now().addingTimeInterval(interval))
        }
        var registry = ConnectorRegistry()
        var unconfigured: [String] = []
        registry.register(ICloudConnectorKind(transport: transport, now: now, sleep: sleep))
        registry.register(CalDAVConnectorKind(transport: transport, now: now, sleep: sleep))
        registry.register(ICalSubscriptionKind(
            transport: transport, now: now, sleep: sleep, defaultZone: defaultZone,
            retention: RetentionWindow(daysBack: 3, daysAhead: 7)))
        // Google's desktop client has a secret as well as an id; both are needed for the token exchange.
        if let google = config.connectors["google"], !google.clientID.isEmpty,
           let secret = google.clientSecret, !secret.isEmpty {
            registry.register(GoogleConnectorKind(
                config: GoogleOAuthConfig(clientID: google.clientID, clientSecret: secret),
                transport: transport, now: now, sleep: sleep))
        } else {
            unconfigured.append(GoogleConnectorKind.kindID)
        }
        if let microsoft = config.connectors["microsoft"], !microsoft.clientID.isEmpty {
            registry.register(MicrosoftConnectorKind(
                config: MicrosoftOAuthConfig(clientID: microsoft.clientID), transport: transport, now: now, sleep: sleep))
        } else {
            unconfigured.append(MicrosoftConnectorKind.kindID)
        }
        return Result(registry: registry, unconfiguredKindIDs: unconfigured)
    }
}
