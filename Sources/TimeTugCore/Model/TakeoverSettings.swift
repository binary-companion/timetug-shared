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
    /// When true, events with no attendees besides the owner never take over. Declined events never do either; that is not a setting.
    public var requireOtherAttendees = false
    /// Hides all-day events from the day list only. All-day events never trigger a takeover, regardless of this setting.
    public var skipAllDayEvents = true
    /// Master switch: while false no takeover or pre-meeting popup fires. The agenda is unaffected.
    public var enabled = true

    public init() {}

    /// Keys written by earlier versions; read only to carry the user's choice forward, never written.
    private enum LegacyKeys: String, CodingKey {
        case disabled, skipSoloEvents
    }

    // Persisted as JSON: every field must decode with decodeIfPresent and its default, so saved settings survive newly added fields.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = TakeoverSettings()
        leadTime = try c.decodeIfPresent(TimeInterval.self, forKey: .leadTime) ?? d.leadTime
        takeoverCalendarKeys = try c.decodeIfPresent(Set<String>.self, forKey: .takeoverCalendarKeys) ?? d.takeoverCalendarKeys
        hiddenCalendarKeys = try c.decodeIfPresent(Set<String>.self, forKey: .hiddenCalendarKeys) ?? d.hiddenCalendarKeys
        requireConferenceLink = try c.decodeIfPresent(Bool.self, forKey: .requireConferenceLink) ?? d.requireConferenceLink
        let legacy = try decoder.container(keyedBy: LegacyKeys.self)
        requireOtherAttendees = try c.decodeIfPresent(Bool.self, forKey: .requireOtherAttendees)
            ?? legacy.decodeIfPresent(Bool.self, forKey: .skipSoloEvents) ?? d.requireOtherAttendees
        skipAllDayEvents = try c.decodeIfPresent(Bool.self, forKey: .skipAllDayEvents) ?? d.skipAllDayEvents
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled)
            ?? legacy.decodeIfPresent(Bool.self, forKey: .disabled).map { !$0 } ?? d.enabled
        // A calendar that is not shown never tugs, so saved data that both hides and opts in a calendar keeps it hidden.
        takeoverCalendarKeys.subtract(hiddenCalendarKeys)
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

    public mutating func setTakeover(_ enabled: Bool, forCalendars keys: [String]) {
        for key in keys { setTakeover(enabled, forCalendar: key) }
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
