import Foundation

/// The `tt_engine_create` configuration.
public struct WireConfig: Codable, Equatable, Sendable {
    public struct ConnectorClient: Codable, Equatable, Sendable {
        public var clientID: String
        public init(clientID: String) { self.clientID = clientID }
    }

    public enum Invalid: Error, Equatable, Sendable {
        case notJSON
        case unsupportedVersion(Int)
        case emptyDataDirectory
        case unknownPlatform(String)
        case unknownTimeZone(String)
    }

    public static let platforms = ["windows", "linux", "macos"]

    public var v: Int
    public var dataDirectory: String
    /// `windows`, `linux` or `macos`.
    public var platform: String
    /// OAuth client ids by kind family (`google`, `microsoft`).
    public var connectors: [String: ConnectorClient]
    public var textGeneration: Bool
    /// An IANA zone id; nil means the process zone.
    public var timeZone: String?
    public var testing: Bool

    public init(
        v: Int = 1, dataDirectory: String, platform: String, connectors: [String: ConnectorClient] = [:],
        textGeneration: Bool = false, timeZone: String? = nil, testing: Bool = false
    ) {
        self.v = v
        self.dataDirectory = dataDirectory
        self.platform = platform
        self.connectors = connectors
        self.textGeneration = textGeneration
        self.timeZone = timeZone
        self.testing = testing
    }

    private enum CodingKeys: String, CodingKey {
        case v, dataDirectory, platform, connectors, textGeneration, timeZone, testing
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        v = try c.decode(Int.self, forKey: .v)
        dataDirectory = try c.decode(String.self, forKey: .dataDirectory)
        platform = try c.decode(String.self, forKey: .platform)
        connectors = try c.decodeIfPresent([String: ConnectorClient].self, forKey: .connectors) ?? [:]
        textGeneration = try c.decodeIfPresent(Bool.self, forKey: .textGeneration) ?? false
        timeZone = try c.decodeIfPresent(String.self, forKey: .timeZone)
        testing = try c.decodeIfPresent(Bool.self, forKey: .testing) ?? false
    }

    /// Decodes and validates. Throws `Invalid` (never a decoding error type) so callers can map it to one result.
    public static func parse(_ json: String) throws -> WireConfig {
        let config: WireConfig
        do { config = try JSONDecoder().decode(WireConfig.self, from: Data(json.utf8)) }
        catch { throw Invalid.notJSON }
        guard ProtocolVersion.supported.contains(config.v) else { throw Invalid.unsupportedVersion(config.v) }
        guard !config.dataDirectory.isEmpty else { throw Invalid.emptyDataDirectory }
        guard platforms.contains(config.platform) else { throw Invalid.unknownPlatform(config.platform) }
        if let zone = config.timeZone, TimeZone(identifier: zone) == nil { throw Invalid.unknownTimeZone(zone) }
        return config
    }
}
