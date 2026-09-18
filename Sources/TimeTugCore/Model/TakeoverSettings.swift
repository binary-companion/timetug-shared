import Foundation

/// Shared, platform-neutral settings. Core defines the shape; each front end stores and edits it.
public struct TakeoverSettings: Codable, Equatable, Sendable {
    /// Seconds before start that the takeover fires. 0 means "starting now".
    public var leadTime: TimeInterval = 60
    /// `CalendarInfo.key` values allowed to trigger takeovers (opt-in).
    public var takeoverCalendarKeys: Set<String> = []
    /// `CalendarInfo.key` values hidden from the day list (default: none hidden).
    public var hiddenCalendarKeys: Set<String> = []
    public var requireConferenceLink = false
    public var skipSoloEvents = true
    public var skipDeclinedEvents = true
    public var skipAllDayEvents = true

    public init() {}

    // Persisted as JSON: every field must decode with decodeIfPresent and its default, so saved settings survive newly added fields.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = TakeoverSettings()
        leadTime = try c.decodeIfPresent(TimeInterval.self, forKey: .leadTime) ?? d.leadTime
        takeoverCalendarKeys = try c.decodeIfPresent(Set<String>.self, forKey: .takeoverCalendarKeys) ?? d.takeoverCalendarKeys
        hiddenCalendarKeys = try c.decodeIfPresent(Set<String>.self, forKey: .hiddenCalendarKeys) ?? d.hiddenCalendarKeys
        requireConferenceLink = try c.decodeIfPresent(Bool.self, forKey: .requireConferenceLink) ?? d.requireConferenceLink
        skipSoloEvents = try c.decodeIfPresent(Bool.self, forKey: .skipSoloEvents) ?? d.skipSoloEvents
        skipDeclinedEvents = try c.decodeIfPresent(Bool.self, forKey: .skipDeclinedEvents) ?? d.skipDeclinedEvents
        skipAllDayEvents = try c.decodeIfPresent(Bool.self, forKey: .skipAllDayEvents) ?? d.skipAllDayEvents
    }
}
