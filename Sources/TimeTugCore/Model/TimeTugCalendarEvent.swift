import Foundation

public struct TimeTugCalendarEvent: Identifiable, Hashable, Sendable {
    public var sourceEventID: String
    public var sourceID: String
    public var calendarID: String
    public var title: String
    public var start: Date
    public var end: Date
    /// All-day events use TimeTug's device-local form: start is the local midnight of the first day, end the local midnight after the last day (exclusive). Sources' own forms are converted by CalendarBridge.
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
    /// Where the range shown to the user starts when it differs from `start` (a merged meeting shows the
    /// longer copy's range while `start` is the tug time); nil means the same as `start`.
    public var displayStart: Date?

    public init(
        sourceEventID: String, sourceID: String, calendarID: String, title: String,
        start: Date, end: Date, isAllDay: Bool = false, otherAttendeeCount: Int = 0,
        responseStatus: ResponseStatus = .unknown, location: String? = nil,
        notes: String? = nil, url: URL? = nil, conferenceURL: URL? = nil,
        additionalCalendarKeys: Set<String> = [],
        attendees: [Attendee] = [], organizerEmail: String? = nil, externalUID: String? = nil,
        mergedMembers: [MergedMember] = [], mergeProvenance: MergeProvenance? = nil,
        displayStart: Date? = nil
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
        self.displayStart = displayStart
    }

    /// The start of the range to display: `displayStart` when set, else `start`.
    public var shownStart: Date { displayStart ?? start }

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
    public func isSameMeeting(as other: TimeTugCalendarEvent) -> Bool {
        id == other.id || !allContentKeys.isDisjoint(with: other.allContentKeys)
    }
}
