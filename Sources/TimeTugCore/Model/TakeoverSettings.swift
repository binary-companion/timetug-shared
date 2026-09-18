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

    public init() {}
}
