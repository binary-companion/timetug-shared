import Foundation
import TimeTugKitGenerated

// `TakeoverSettings` of TimeTugCore 1.x: the defaults, the `Codable` form the Mac app keeps in UserDefaults, and the
// mutators. The generated struct has the fields and `Equatable`/`Hashable`.

extension TakeoverSettings: Codable {
    /// The 1.x defaults: 60 seconds before the start, no calendar opted in or hidden, all-day events skipped, enabled.
    public init() {
        self.init(
            leadTime: 60, takeoverCalendarKeys: [], hiddenCalendarKeys: [], requireConferenceLink: false,
            requireOtherAttendees: false, skipAllDayEvents: true, enabled: true)
    }

    private enum CodingKeys: String, CodingKey {
        case leadTime, takeoverCalendarKeys, hiddenCalendarKeys, requireConferenceLink, requireOtherAttendees
        case skipAllDayEvents, enabled
    }

    /// Keys written by earlier versions; read only to carry the user's choice forward, never written.
    private enum LegacyKeys: String, CodingKey {
        case disabled, skipSoloEvents
    }

    // Persisted as JSON: every field decodes with `decodeIfPresent` and its default, so saved settings survive newly added
    // fields (the 1.x decoder, key for key).
    public init(from decoder: Decoder) throws {
        self.init()
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

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(leadTime, forKey: .leadTime)
        try c.encode(takeoverCalendarKeys, forKey: .takeoverCalendarKeys)
        try c.encode(hiddenCalendarKeys, forKey: .hiddenCalendarKeys)
        try c.encode(requireConferenceLink, forKey: .requireConferenceLink)
        try c.encode(requireOtherAttendees, forKey: .requireOtherAttendees)
        try c.encode(skipAllDayEvents, forKey: .skipAllDayEvents)
        try c.encode(enabled, forKey: .enabled)
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
