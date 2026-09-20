import CalendarCore
import Foundation
import TimeTugCore

/// Maps the connector library's model to TimeTug's. The only place that knows both vocabularies.
public struct EventMapper: Sendable {
    private let calendar: Calendar

    /// `calendar` decides which day an all-day event falls on (the user's own calendar).
    public init(calendar: Calendar = .autoupdatingCurrent) { self.calendar = calendar }

    public func calendarInfo(_ d: CalendarDescriptor, sourceID: String) -> CalendarInfo {
        CalendarInfo(sourceID: sourceID, calendarID: d.id, title: d.title, accountName: d.accountName, colorHex: d.colorHex)
    }

    /// nil for cancelled events. Titles are never rewritten: each connector chooses its own placeholder.
    public func event(_ e: CalendarCore.CalendarEvent, sourceID: String) -> TimeTugCalendarEvent? {
        if e.status == .cancelled { return nil }
        let (start, end) = times(e)
        let others = e.attendees.filter { !$0.isSelf }
        return TimeTugCalendarEvent(
            sourceEventID: e.eventID, sourceID: sourceID, calendarID: e.calendarID, title: e.title,
            start: start, end: end, isAllDay: e.isAllDay, otherAttendeeCount: others.count,
            responseStatus: responseStatus(of: e), location: e.location, notes: e.notes, url: e.url,
            conferenceURL: e.conference?.url,
            attendees: others.map { TimeTugCore.Attendee(name: $0.name, email: $0.email) },
            organizerEmail: e.organizer.flatMap { $0.isSelf ? nil : $0.email },
            externalUID: e.uid)
    }

    /// INTERIM (Phase 2.5 deletes this): the library's all-day events are midnights in the event's own zone;
    /// TimeTug's agenda still expects device-local midnights of the same calendar dates. Identity when the zones match.
    private func times(_ e: CalendarCore.CalendarEvent) -> (Date, Date) {
        guard e.isAllDay, let zone = e.timeZone else { return (e.start, e.end) }
        let days = AllDay.dates(start: e.start, end: e.end, in: zone)
        let local = calendar.timeZone
        guard let start = AllDay.startOfDay(days.first, in: local),
              let end = AllDay.startOfDay(days.endExclusive, in: local) else { return (e.start, e.end) }
        return (start, end)
    }

    private func responseStatus(of e: CalendarCore.CalendarEvent) -> TimeTugCore.ResponseStatus {
        switch e.myResponse ?? e.attendees.first(where: \.isSelf)?.response {
        case .accepted: .accepted
        case .tentative: .tentative
        case .declined: .declined
        case .needsAction: .pending
        case nil: .unknown
        }
    }
}
