import Foundation

public enum ResponseStatus: String, Codable, Sendable {
    case accepted, tentative, declined, pending, unknown
}

public struct CalendarInfo: Hashable, Sendable, Identifiable {
    public let sourceID: String
    public let calendarID: String
    public let title: String

    public init(sourceID: String, calendarID: String, title: String) {
        self.sourceID = sourceID
        self.calendarID = calendarID
        self.title = title
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

    public init(
        sourceEventID: String, sourceID: String, calendarID: String, title: String,
        start: Date, end: Date, isAllDay: Bool = false, otherAttendeeCount: Int = 0,
        responseStatus: ResponseStatus = .unknown, location: String? = nil,
        notes: String? = nil, url: URL? = nil, conferenceURL: URL? = nil,
        additionalCalendarKeys: Set<String> = []
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
    }

    /// Unique per occurrence: recurring events share a source id but differ in start.
    public var id: String { "\(sourceID)/\(sourceEventID)/\(Int(start.timeIntervalSince1970))" }
    public var calendarKey: String { CalendarInfo.key(sourceID: sourceID, calendarID: calendarID) }
    /// This event's own calendar plus every calendar its duplicates appear on.
    public var allCalendarKeys: Set<String> { additionalCalendarKeys.union([calendarKey]) }
}
