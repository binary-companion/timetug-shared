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
    /// Hides all-day events from the day list only. All-day events never trigger a takeover, regardless of this setting.
    public var skipAllDayEvents = true
    /// Master switch: while true no takeover or pre-meeting popup fires. The agenda is unaffected.
    public var disabled = false

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
        disabled = try c.decodeIfPresent(Bool.self, forKey: .disabled) ?? d.disabled
        // Legacy data may hide a takeover calendar; takeover wins so a meeting alert is never silently lost.
        hiddenCalendarKeys.subtract(takeoverCalendarKeys)
    }

    public func isShownInList(_ key: String) -> Bool {
        !hiddenCalendarKeys.contains(key)
    }

    /// Enabling takeover also shows the calendar in the list; disabling leaves visibility alone.
    public mutating func setTakeover(_ enabled: Bool, forCalendar key: String) {
        if enabled {
            takeoverCalendarKeys.insert(key)
            hiddenCalendarKeys.remove(key)
        } else {
            takeoverCalendarKeys.remove(key)
        }
    }

    /// Hiding a calendar also turns takeover off for it; showing leaves takeover alone.
    public mutating func setShownInList(_ shown: Bool, forCalendar key: String) {
        if shown {
            hiddenCalendarKeys.remove(key)
        } else {
            hiddenCalendarKeys.insert(key)
            takeoverCalendarKeys.remove(key)
        }
    }

    /// Removes every stored calendar key whose source id (the text before the first "/") satisfies `isRemoved`.
    public mutating func removeCalendars(whereSourceID isRemoved: (String) -> Bool) {
        func sourceID(of key: String) -> String {
            key.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init) ?? key
        }
        takeoverCalendarKeys = takeoverCalendarKeys.filter { !isRemoved(sourceID(of: $0)) }
        hiddenCalendarKeys = hiddenCalendarKeys.filter { !isRemoved(sourceID(of: $0)) }
    }

    /// Forgets the Tug and visibility choices of one source (a removed account).
    public mutating func removeCalendars(forSourceID sourceID: String) {
        removeCalendars(whereSourceID: { $0 == sourceID })
    }
}
