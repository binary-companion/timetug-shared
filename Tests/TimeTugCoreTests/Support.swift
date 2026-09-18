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
    status: ResponseStatus = .accepted,
    location: String? = nil,
    notes: String? = nil,
    url: URL? = nil,
    conferenceURL: URL? = nil
) -> CalendarEvent {
    let startDate = date(start)
    return CalendarEvent(
        sourceEventID: id, sourceID: "fake", calendarID: calendarID, title: title,
        start: startDate, end: startDate.addingTimeInterval(TimeInterval(minutes * 60)),
        isAllDay: isAllDay, otherAttendeeCount: others, responseStatus: status,
        location: location, notes: notes, url: url, conferenceURL: conferenceURL
    )
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
