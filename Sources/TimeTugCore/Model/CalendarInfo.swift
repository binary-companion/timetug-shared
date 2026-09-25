import CalendarCore
import Foundation

public struct CalendarInfo: Hashable, Sendable, Identifiable {
    public let sourceID: String
    public let calendarID: String
    public let title: String
    /// Owning account (iCloud, Google, ...), for grouping. Not part of `key`.
    public let accountName: String?
    /// Calendar color as "#RRGGBB" (uppercase, sRGB); nil when unknown. Not part of `key`.
    public let colorHex: String?
    /// Birthdays and subscribed feeds are system-style calendars. Descriptive only; not part of `key`.
    public let kind: CalendarKind
    /// The connector the calendar is read through, and who hosts it (nil when the source cannot tell). Descriptive
    /// only; not part of `key`. Lets Core see which calendars are the same account across sources.
    public let service: CalendarService?
    public let provider: CalendarProvider?

    public init(
        sourceID: String, calendarID: String, title: String,
        accountName: String? = nil, colorHex: String? = nil, kind: CalendarKind = .standard,
        service: CalendarService? = nil, provider: CalendarProvider? = nil
    ) {
        self.sourceID = sourceID
        self.calendarID = calendarID
        self.title = title
        self.accountName = accountName
        self.colorHex = Self.normalizedHex(colorHex)
        self.kind = kind
        self.service = service
        self.provider = provider
    }

    /// Normalizes "#RGB", "#RRGGBB" or "RRGGBB" (any case, surrounding whitespace ok)
    /// to "#RRGGBB" uppercase; nil for anything invalid.
    public static func normalizedHex(_ raw: String?) -> String? {
        guard var s = raw?.trimmingCharacters(in: .whitespacesAndNewlines) else { return nil }
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 3 || s.count == 6, s.allSatisfy(\.isASCII), s.allSatisfy(\.isHexDigit) else { return nil }
        s = s.uppercased()
        if s.count == 3 { s = s.map { "\($0)\($0)" }.joined() }
        return "#" + s
    }

    public var key: String { Self.key(sourceID: sourceID, calendarID: calendarID) }
    public var id: String { key }

    public static func key(sourceID: String, calendarID: String) -> String {
        "\(sourceID)/\(calendarID)"
    }
}
