import Foundation

/// Every message type of protocol v1, in one place: the documentation table and the golden-fixture test both read
/// these lists, so a type cannot be added without a fixture.
public enum MessageCatalog {
    public static let hostToEngine: [String] = [
        "start", "refresh",
        "account.add", "account.reconnect", "account.remove", "account.cancel",
        "settings.apply", "countdown.visible",
        "takeover.presented", "takeover.closed", "takeover.join", "takeover.dismiss", "takeover.snooze", "takeover.test",
        "dedup.merge", "dedup.unmerge", "dedup.separate", "dedup.forget",
        "system.event", "test.advanceClock",
    ]

    public static let engineToHost: [String] = [
        "ready", "state",
        "event.takeoverDue", "event.takeoverWithdrawn", "event.widgetSnapshot", "event.joinRequested",
        "event.diagnostic",
    ]

    public static let services: [String] = ServiceMessages.types

    public static var all: [String] { hostToEngine + engineToHost + services }
}

/// Payload of the first message the engine sends after `start`.
public struct WireReady: Codable, Equatable, Sendable {
    public struct Capabilities: Codable, Equatable, Sendable {
        public var systemCalendar: Bool
        public var textGeneration: Bool
        public init(systemCalendar: Bool, textGeneration: Bool) {
            self.systemCalendar = systemCalendar
            self.textGeneration = textGeneration
        }
    }

    public var engineVersion: String
    public var protocolVersions: [Int]
    public var platform: String
    public var capabilities: Capabilities

    public init(engineVersion: String, protocolVersions: [Int] = ProtocolVersion.supported, platform: String,
                capabilities: Capabilities) {
        self.engineVersion = engineVersion
        self.protocolVersions = protocolVersions
        self.platform = platform
        self.capabilities = capabilities
    }
}
