import Foundation

public enum ResponseStatus: String, Codable, Sendable {
    case accepted, tentative, declined, pending, unknown
}

public struct CalendarInfo: Hashable, Sendable, Identifiable {
    public let sourceID: String
    public let calendarID: String
    public let title: String
    /// Owning account (iCloud, Google, ...), for grouping. Not part of `key`.
    public let accountName: String?
    /// Calendar color as "#RRGGBB" (uppercase, sRGB); nil when unknown. Not part of `key`.
    public let colorHex: String?

    public init(
        sourceID: String, calendarID: String, title: String,
        accountName: String? = nil, colorHex: String? = nil
    ) {
        self.sourceID = sourceID
        self.calendarID = calendarID
        self.title = title
        self.accountName = accountName
        self.colorHex = Self.normalizedHex(colorHex)
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

public struct CalendarEvent: Identifiable, Hashable, Sendable {
    public var sourceEventID: String
    public var sourceID: String
    public var calendarID: String
    public var title: String
    public var start: Date
    public var end: Date
    public var isAllDay: Bool
    public var otherAttendeeCount: Int
    public var responseStatus: ResponseStatus
    public var location: String?
    public var notes: String?
    public var url: URL?
    public var conferenceURL: URL?
    /// Calendar keys of the other calendars where duplicate copies of this meeting appear.
    public var additionalCalendarKeys: Set<String>
    public var attendees: [Attendee]
    public var organizerEmail: String?
    public var externalUID: String?
    /// Every original copy folded into this event (including itself); empty when never merged.
    public var mergedMembers: [MergedMember]
    public var mergeProvenance: MergeProvenance?

    public init(
        sourceEventID: String, sourceID: String, calendarID: String, title: String,
        start: Date, end: Date, isAllDay: Bool = false, otherAttendeeCount: Int = 0,
        responseStatus: ResponseStatus = .unknown, location: String? = nil,
        notes: String? = nil, url: URL? = nil, conferenceURL: URL? = nil,
        additionalCalendarKeys: Set<String> = [],
        attendees: [Attendee] = [], organizerEmail: String? = nil, externalUID: String? = nil,
        mergedMembers: [MergedMember] = [], mergeProvenance: MergeProvenance? = nil
    ) {
        self.sourceEventID = sourceEventID
        self.sourceID = sourceID
        self.calendarID = calendarID
        self.title = title
        self.start = start
        self.end = end
        self.isAllDay = isAllDay
        self.otherAttendeeCount = otherAttendeeCount
        self.responseStatus = responseStatus
        self.location = location
        self.notes = notes
        self.url = url
        self.conferenceURL = conferenceURL
        self.additionalCalendarKeys = additionalCalendarKeys
        self.attendees = attendees
        self.organizerEmail = organizerEmail
        self.externalUID = externalUID
        self.mergedMembers = mergedMembers
        self.mergeProvenance = mergeProvenance
    }

    /// Unique per occurrence: recurring events share a source id but differ in start.
    public var id: String { "\(sourceID)/\(sourceEventID)/\(Int(start.timeIntervalSince1970))" }
    /// Identity by content (title, start, end): survives a changed `sourceEventID`, matching the
    /// store's duplicate merge.
    public var contentKey: String {
        "\(title.lowercased())|\(Int(start.timeIntervalSince1970))|\(Int(end.timeIntervalSince1970))"
    }
    public var calendarKey: String { CalendarInfo.key(sourceID: sourceID, calendarID: calendarID) }
    /// This event's own calendar plus every calendar its duplicates appear on.
    public var allCalendarKeys: Set<String> { additionalCalendarKeys.union([calendarKey]) }

    /// Content keys of this event and every copy merged into it.
    public var allContentKeys: Set<String> { Set(mergedMembers.map(\.contentKey)).union([contentKey]) }

    /// True for the same occurrence or when any merged copy's content matches (an armed timer's
    /// event may since have been merged into another, or split back out).
    public func isSameMeeting(as other: CalendarEvent) -> Bool {
        id == other.id || !allContentKeys.isDisjoint(with: other.allContentKeys)
    }
}
