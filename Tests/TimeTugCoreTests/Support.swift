import CalendarCore
import Foundation
@testable import TimeTugCore

let utcCalendar: Calendar = {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "UTC")!
    return calendar
}()

/// Parses "2026-09-18T09:00:00Z".
func date(_ iso: String) -> Date {
    ISO8601DateFormatter().date(from: iso)!
}

func makeEvent(
    _ id: String = "e1",
    title: String = "Standup",
    start: String = "2026-09-18T10:00:00Z",
    minutes: Int = 30,
    calendarID: String = "cal",
    isAllDay: Bool = false,
    others: Int = 1,
    status: CalendarCore.ResponseStatus? = .accepted,
    location: String? = nil,
    notes: String? = nil,
    url: URL? = nil,
    conferenceURL: URL? = nil,
    attendees: [CalendarCore.Attendee] = [],
    externalUID: String? = nil
) -> TimeTugCalendarEvent {
    let startDate = date(start)
    let base = CalendarCore.CalendarEvent(
        eventID: id, uid: externalUID, calendarID: calendarID, title: title, notes: notes, location: location,
        start: startDate, end: startDate.addingTimeInterval(TimeInterval(minutes * 60)),
        timeZone: isAllDay ? TimeZone(identifier: "UTC")! : nil, isAllDay: isAllDay, attendees: attendees, url: url)
    var event = TimeTugCalendarEvent(event: base, sourceID: "fake")
    event.otherAttendeeCount = others
    event.responseStatus = status
    event.conferenceURL = conferenceURL
    return event
}

/// Settings with the default test calendar ("fake/cal") opted in for takeovers.
func optedIn(_ configure: (inout TakeoverSettings) -> Void = { _ in }) -> TakeoverSettings {
    var settings = TakeoverSettings()
    settings.takeoverCalendarKeys = ["fake/cal"]
    configure(&settings)
    return settings
}

/// A "now" at which test ledger entries are recorded (before the default test events).
let recordedAt = date("2026-09-18T09:00:00Z")

func calendar(in zone: String) -> Calendar {
    var c = Calendar(identifier: .gregorian)
    c.timeZone = TimeZone(identifier: zone)!
    return c
}

/// A canonical all-day event: `first` to `endExclusive` (calendar dates) in `zone`.
func makeAllDay(_ id: String = "d", zone: String, first: CalendarDate, endExclusive: CalendarDate,
                title: String = "Holiday", calendarID: String = "cal") -> TimeTugCalendarEvent {
    let tz = TimeZone(identifier: zone)!
    let range = AllDay.canonical(first: first, endExclusive: endExclusive, in: tz)!
    let base = CalendarCore.CalendarEvent(
        eventID: id, calendarID: calendarID, title: title, start: range.start, end: range.end, timeZone: tz, isAllDay: true)
    return TimeTugCalendarEvent(event: base, sourceID: "fake")
}

func day(_ y: Int, _ m: Int, _ d: Int) -> CalendarDate { CalendarDate(year: y, month: m, day: d) }
