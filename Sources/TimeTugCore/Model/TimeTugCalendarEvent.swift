import CalendarCore
import Foundation

/// A library event as TimeTug sees it: the provider's event plus the source it came from and the state Core adds
/// (merged duplicates, the join link Core detects, the best attendance across copies). Times, all-day form and
/// attendees are the library's own; nothing is converted per refresh.
public struct TimeTugCalendarEvent: Identifiable, Hashable, Sendable {
    public var event: CalendarCore.CalendarEvent
    public var sourceID: String
    /// The join links, most likely first: the library's own list (structured links, then links found in the text),
    /// merged across copies by the duplicate resolver.
    public var conferences: [ConferenceInfo]
    /// The first join link.
    public var conferenceURL: URL? { conferences.first?.url }
    /// Non-self attendees; the merge step raises it to the largest count across a group.
    public var otherAttendeeCount: Int
    /// The account owner's response; nil when the provider does not say. The merge step keeps the best across a group.
    public var responseStatus: CalendarCore.ResponseStatus?
    /// Calendar keys of the other calendars where duplicate copies of this meeting appear.
    public var additionalCalendarKeys: Set<String>
    /// Every original copy folded into this event (including itself); empty when never merged.
    public var mergedMembers: [MergedMember]
    public var mergeProvenance: MergeProvenance?
    /// Where the range shown to the user starts when it differs from `start` (a merged meeting shows the
    /// longer copy's range while `start` is the tug time); nil means the same as `start`.
    public var displayStart: Date?

    public init(event: CalendarCore.CalendarEvent, sourceID: String) {
        self.event = event
        self.sourceID = sourceID
        self.conferences = event.conferences
        self.otherAttendeeCount = event.attendees.filter { !$0.isSelf }.count
        self.responseStatus = event.myResponse ?? event.attendees.first(where: \.isSelf)?.response
        self.additionalCalendarKeys = []
        self.mergedMembers = []
        self.mergeProvenance = nil
        self.displayStart = nil
    }

    public var title: String { get { event.title } set { event.title = newValue } }
    public var start: Date { get { event.start } set { event.start = newValue } }
    public var end: Date { get { event.end } set { event.end = newValue } }
    /// All-day events use the library's canonical form: midnight of the first day in `timeZone`, `end` exclusive.
    public var isAllDay: Bool { get { event.isAllDay } set { event.isAllDay = newValue } }
    public var timeZone: TimeZone { get { event.timeZone } set { event.timeZone = newValue } }
    public var location: String? { get { event.location } set { event.location = newValue } }
    public var notes: String? { get { event.notes } set { event.notes = newValue } }
    public var url: URL? { get { event.url } set { event.url = newValue } }
    public var calendarID: String { get { event.calendarID } set { event.calendarID = newValue } }

    public var sourceEventID: String { event.eventID }
    public var externalUID: String? { event.uid }
    /// Attendees other than the calendar owner.
    public var attendees: [CalendarCore.Attendee] { event.attendees.filter { !$0.isSelf } }
    public var organizerEmail: String? { event.organizer.flatMap { $0.isSelf ? nil : $0.email } }

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
