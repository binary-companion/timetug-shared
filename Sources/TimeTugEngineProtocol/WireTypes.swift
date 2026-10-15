import Foundation

/// Instants and calendar dates as the wire writes them. Hand-formatted (no `ISO8601DateFormatter`, whose options
/// differ between Darwin and swift-corelibs-foundation) so Mac, Linux and Windows emit the same bytes.
public enum WireDates {
    /// `2025-10-09T08:53:20.123Z`: UTC, millisecond precision (rounded to the nearest millisecond).
    public static func string(_ date: Date) -> String {
        let totalMillis = Int64((date.timeIntervalSince1970 * 1000).rounded())
        let seconds = floorDiv(totalMillis, 1000)
        let millis = Int(totalMillis - seconds * 1000)
        let days = floorDiv(seconds, 86_400)
        let secondOfDay = Int(seconds - days * 86_400)
        let (y, m, d) = civil(fromDays: days)
        return "\(pad(y, 4))-\(pad(m, 2))-\(pad(d, 2))T\(pad(secondOfDay / 3600, 2)):\(pad(secondOfDay / 60 % 60, 2)):"
            + "\(pad(secondOfDay % 60, 2)).\(pad(millis, 3))Z"
    }

    /// Reads the format `string` writes (fraction optional). nil for anything else.
    public static func date(_ text: String) -> Date? {
        let scalars = Array(text.utf8)
        func num(_ range: Range<Int>) -> Int? {
            guard range.upperBound <= scalars.count else { return nil }
            var value = 0
            for i in range {
                guard scalars[i] >= 48, scalars[i] <= 57 else { return nil }
                value = value * 10 + Int(scalars[i] - 48)
            }
            return value
        }
        guard scalars.count >= 20, scalars[4] == 45, scalars[7] == 45, scalars[10] == 84, scalars[13] == 58,
              scalars[16] == 58, scalars.last == 90,
              let y = num(0..<4), let mo = num(5..<7), let d = num(8..<10),
              let h = num(11..<13), let mi = num(14..<16), let s = num(17..<19),
              (1...12).contains(mo), (1...31).contains(d), h < 24, mi < 60, s < 61 else { return nil }
        var fraction = 0.0
        if scalars.count > 20 {
            // At most nine fraction digits: more cannot matter for a Date and would overflow `num`.
            guard scalars[19] == 46, scalars.count > 21, scalars.count <= 30,
                  let f = num(20..<(scalars.count - 1)) else { return nil }
            fraction = Double(f) / pow(10, Double(scalars.count - 21))
        } else if scalars[19] != 90 { return nil }
        let days = daysFromCivil(y, mo, d)
        // Reject impossible dates ("2025-02-30"): the round trip must give back the same day.
        guard civil(fromDays: days) == (y, mo, d) else { return nil }
        return Date(timeIntervalSince1970: Double(days * 86_400 + Int64(h * 3600 + mi * 60 + s)) + fraction)
    }

    /// `YYYY-MM-DD`.
    public static func dateString(year: Int, month: Int, day: Int) -> String {
        "\(pad(year, 4))-\(pad(month, 2))-\(pad(day, 2))"
    }

    private static func pad(_ value: Int, _ width: Int) -> String {
        let s = String(value)
        return String(repeating: "0", count: max(0, width - s.count)) + s
    }

    private static func floorDiv(_ a: Int64, _ b: Int64) -> Int64 {
        let q = a / b
        return (a % b != 0 && (a < 0) != (b < 0)) ? q - 1 : q
    }

    // Howard Hinnant's civil-date algorithms (proleptic Gregorian, days since 1970-01-01).
    private static func civil(fromDays days: Int64) -> (Int, Int, Int) {
        let z = days + 719_468
        let era = floorDiv(z, 146_097)
        let doe = z - era * 146_097
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100)
        let mp = (5 * doy + 2) / 153
        let d = doy - (153 * mp + 2) / 5 + 1
        let m = mp < 10 ? mp + 3 : mp - 9
        let y = yoe + era * 400 + (m <= 2 ? 1 : 0)
        return (Int(y), Int(m), Int(d))
    }

    private static func daysFromCivil(_ y: Int, _ m: Int, _ d: Int) -> Int64 {
        let yy = Int64(m <= 2 ? y - 1 : y)
        let era = floorDiv(yy, 400)
        let yoe = yy - era * 400
        let mp = Int64(m > 2 ? m - 3 : m + 9)
        let doy = (153 * mp + 2) / 5 + Int64(d) - 1
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
        return era * 146_097 + doe - 719_468
    }
}

// MARK: - Event DTOs

/// A merged event's original copy. `memberID` is the copy's `contentKey` (title, start and end, lowercased and
/// second-resolution): the engine keeps no per-copy event id, and `dedup.separate` resolves its `memberIDs`
/// against the merged event's own member list by exactly this key.
public struct WireMember: Codable, Equatable, Sendable {
    public var memberID: String
    public var calendarKey: String
    public var start: String
    public var end: String
    public var details: String
}

public struct WireConference: Codable, Equatable, Sendable {
    public var url: String
    /// `meet`, `teams`, `zoom`, `webex`, `goToMeeting`, `whereby`, `jitsi`, `slack` or `other`.
    public var provider: String
}

public struct WireProvenance: Codable, Equatable, Sendable {
    /// `rule`, `inference` or `userConfirmed`.
    public var kind: String
    public var engineID: String?
    public var engineName: String?
}

/// One event. A timed event carries `start` and `end` instants; an all-day event carries `startDate` and
/// `endDate` (exclusive) read in `timeZone`, and no instants.
public struct WireEvent: Codable, Equatable, Sendable {
    public var id: String
    public var sourceID: String
    public var sourceEventID: String
    public var calendarKey: String
    public var title: String
    public var isAllDay: Bool
    public var start: String?
    public var end: String?
    public var startDate: String?
    public var endDate: String?
    /// Where the shown range starts when it differs from `start` (a merged meeting); timed events only.
    public var displayStart: String?
    public var timeZone: String
    public var location: String?
    public var url: String?
    /// `confirmed`, `tentative` or `cancelled`.
    public var status: String
    public var conferences: [WireConference]
    public var otherAttendeeCount: Int
    /// `accepted`, `tentative`, `declined` or `needsAction`; absent when the provider does not say.
    public var responseStatus: String?
    public var additionalCalendarKeys: [String]
    public var members: [WireMember]
    public var mergeProvenance: WireProvenance?
}

public struct WireAgendaItem: Codable, Equatable, Sendable {
    public var eventID: String
    /// `past`, `current` or `upcoming`.
    public var state: String
}

public struct WireAgenda: Codable, Equatable, Sendable {
    public var items: [WireAgendaItem]
    public var nextEventID: String?
}

public struct WireCalendar: Codable, Equatable, Sendable {
    public var key: String
    public var sourceID: String
    public var calendarID: String
    public var title: String
    public var accountName: String?
    public var colorHex: String?
    /// `standard`, `subscribed` or `birthdays`.
    public var kind: String
    public var service: String?
    public var provider: String?
}

public struct WireSourceStatus: Codable, Equatable, Sendable {
    /// `ok`, `needsPermission`, `authExpired` or `failing`.
    public var kind: String
    public var message: String?
}

public struct WireNotice: Codable, Equatable, Sendable {
    public var kind: String
    public var count: Int
}

public struct WireAccount: Codable, Equatable, Sendable {
    public var kindID: String
    public var connectionID: String
    public var displayName: String
    public var config: [String: String]
}

public struct WireAccountStatus: Codable, Equatable, Sendable {
    public var isWorking: Bool
    public var buildFailures: [String: String]
    public var errorMessage: String?
    /// `browser` or `inApp`.
    public var waitingStyle: String
}

public struct WireAccountKind: Codable, Equatable, Sendable {
    public var id: String
    public var displayName: String
    /// `oauth`, `password` or `system`.
    public var authorization: String
    public var isConfigured: Bool
}

public struct WireInferenceStatus: Codable, Equatable, Sendable {
    /// `disabled`, `noEngine`, `unavailable`, `notOnDevice` or `active`.
    public var kind: String
    public var reason: String?
    public var engineID: String?
    public var engineName: String?
    public var isOnDevice: Bool?
}

public struct WireLedger: Codable, Equatable, Sendable {
    public var count: Int
}

public struct WireNextTakeover: Codable, Equatable, Sendable {
    public var eventID: String
    public var fireAt: String
}

// MARK: - Settings

public struct WireTakeoverSettings: Codable, Equatable, Sendable {
    public var leadTime: Double
    public var takeoverCalendarKeys: [String]
    public var hiddenCalendarKeys: [String]
    public var requireConferenceLink: Bool
    public var requireOtherAttendees: Bool
    public var skipAllDayEvents: Bool
    public var enabled: Bool
}

public struct WireSettings: Codable, Equatable, Sendable {
    public var takeover: WireTakeoverSettings
    public var inferenceEnabled: Bool
    public var systemCalendarEnabled: Bool
}

/// `settings.apply` payload: every field optional, absent means unchanged. The takeover fields sit flat beside
/// the two top-level switches; a calendar key list replaces the stored list.
public struct WireSettingsPatch: Codable, Equatable, Sendable {
    public var leadTime: Double?
    public var takeoverCalendarKeys: [String]?
    public var hiddenCalendarKeys: [String]?
    public var requireConferenceLink: Bool?
    public var requireOtherAttendees: Bool?
    public var skipAllDayEvents: Bool?
    public var enabled: Bool?
    public var inferenceEnabled: Bool?
    public var systemCalendarEnabled: Bool?

    public init() {}
}

// MARK: - State and events

/// The whole `EngineState`, field for field. `fieldNames` is the coding-key list itself, so a test can compare it
/// with the engine type's stored properties and a later engine field cannot be forgotten.
public struct WireState: Codable, Equatable, Sendable {
    public var revision: UInt64
    public var agenda: WireAgenda
    public var events: [WireEvent]
    public var calendars: [WireCalendar]
    public var statuses: [String: WireSourceStatus]
    public var notices: [String: [WireNotice]]
    public var sourceNames: [String: String]
    /// Pending duplicate pairs: an event id to the ids of the events it may duplicate; all resolve in `events`.
    public var candidates: [String: [String]]
    public var accounts: [WireAccount]
    public var accountStatus: WireAccountStatus
    public var settings: WireSettings
    public var inferenceStatus: WireInferenceStatus
    public var ledger: WireLedger
    public var nextTakeover: WireNextTakeover?
    public var takeoverVisible: Bool
    public var availableAccountKinds: [WireAccountKind]

    enum CodingKeys: String, CodingKey, CaseIterable {
        case revision, agenda, events, calendars, statuses, notices, sourceNames, candidates, accounts
        case accountStatus, settings, inferenceStatus, ledger, nextTakeover, takeoverVisible, availableAccountKinds
    }

    public static var fieldNames: [String] { CodingKeys.allCases.map(\.rawValue) }
}

public struct WireTakeoverRequest: Codable, Equatable, Sendable {
    public var event: WireEvent
    public var hasStarted: Bool
    public var joinURL: String?
    /// Seconds.
    public var snoozeOptions: [Double]
    public var calendarTitle: String?
}

public struct WireWidgetEvent: Codable, Equatable, Sendable {
    public var id: String
    public var title: String
    public var start: String
    public var end: String
    public var isAllDay: Bool
    public var colorHex: String?
    public var joinURL: String?
}

/// A view model for widgets: all-day events carry the local midnights of their dates, as instants.
public struct WireWidgetSnapshot: Codable, Equatable, Sendable {
    public var generatedAt: String
    public var events: [WireWidgetEvent]
}

public struct WireDiagnostic: Codable, Equatable, Sendable {
    /// `info`, `warning` or `error`.
    public var level: String
    public var category: String
    public var message: String
}
