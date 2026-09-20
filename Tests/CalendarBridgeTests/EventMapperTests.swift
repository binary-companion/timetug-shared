import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import CalendarBridge

private func zone(_ id: String) -> TimeZone { TimeZone(identifier: id)! }
private func iso(_ s: String) -> Date { ISO8601DateFormatter().date(from: s)! }
private func mapper(in id: String = "America/New_York") -> EventMapper {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = zone(id)
    return EventMapper(calendar: calendar)
}
private func timed(_ configure: (inout CalendarCore.CalendarEvent) -> Void = { _ in }) -> CalendarCore.CalendarEvent {
    var e = CalendarCore.CalendarEvent(
        eventID: "e1", uid: "uid-1", calendarID: "cal", title: "Standup", notes: "n", location: "Room 1",
        start: iso("2026-09-18T14:00:00Z"), end: iso("2026-09-18T14:30:00Z"), url: URL(string: "https://x.test/e"))
    configure(&e)
    return e
}
private func allDay(first: (Int, Int, Int), endExclusive: (Int, Int, Int), zone id: String) -> CalendarCore.CalendarEvent {
    let z = zone(id)
    let r = AllDay.canonical(
        first: CalendarDate(year: first.0, month: first.1, day: first.2),
        endExclusive: CalendarDate(year: endExclusive.0, month: endExclusive.1, day: endExclusive.2), in: z)!
    return CalendarCore.CalendarEvent(eventID: "d", calendarID: "cal", title: "Holiday", start: r.start, end: r.end,
                                      timeZone: z, isAllDay: true)
}

@Test func mapsTimedEventFields() throws {
    let e = try #require(mapper().event(timed { $0.conference = ConferenceInfo(url: URL(string: "https://meet.example/x")!, provider: .meet) }, sourceID: "google-1"))
    #expect(e.sourceEventID == "e1" && e.sourceID == "google-1" && e.calendarID == "cal" && e.title == "Standup")
    #expect(e.start == iso("2026-09-18T14:00:00Z") && e.end == iso("2026-09-18T14:30:00Z") && !e.isAllDay)
    #expect(e.location == "Room 1" && e.notes == "n" && e.url == URL(string: "https://x.test/e"))
    #expect(e.conferenceURL == URL(string: "https://meet.example/x") && e.externalUID == "uid-1")
}

@Test func separatesSelfFromOtherAttendeesAndOrganizer() throws {
    let e = try #require(mapper().event(timed {
        $0.attendees = [
            CalendarCore.Attendee(name: "Me", email: "me@x.test", response: .accepted, isSelf: true),
            CalendarCore.Attendee(name: "Bo", email: "BO@x.test"),
        ]
        $0.organizer = CalendarCore.Attendee(name: "Bo", email: "bo@x.test", isOrganizer: true)
    }, sourceID: "s"))
    #expect(e.otherAttendeeCount == 1)
    #expect(e.attendees.map(\.email) == ["bo@x.test"])
    #expect(e.organizerEmail == "bo@x.test")
    #expect(e.responseStatus == .accepted)
}

@Test func organizerWhoIsSelfIsNotReported() throws {
    let e = try #require(mapper().event(timed { $0.organizer = CalendarCore.Attendee(email: "me@x.test", isSelf: true, isOrganizer: true) }, sourceID: "s"))
    #expect(e.organizerEmail == nil)
}

@Test func responseStatusMapping() throws {
    func status(_ configure: (inout CalendarCore.CalendarEvent) -> Void) throws -> TimeTugCore.ResponseStatus {
        try #require(mapper().event(timed(configure), sourceID: "s")).responseStatus
    }
    #expect(try status { $0.myResponse = .declined } == .declined)
    #expect(try status { $0.myResponse = .tentative } == .tentative)
    #expect(try status { $0.myResponse = .needsAction } == .pending)
    #expect(try status { $0.attendees = [CalendarCore.Attendee(email: "me@x.test", response: .declined, isSelf: true)] } == .declined)
    #expect(try status { _ in } == .unknown)
}

@Test func cancelledEventsAreDropped() {
    #expect(mapper().event(timed { $0.status = .cancelled }, sourceID: "s") == nil)
}

@Test func titlesPassThroughUnchangedIncludingEmpty() throws {
    #expect(try #require(mapper().event(timed { $0.title = "" }, sourceID: "s")).title == "")
}

@Test func allDayInTheDeviceZoneIsTheIdentity() throws {
    let source = allDay(first: (2026, 9, 18), endExclusive: (2026, 9, 19), zone: "America/New_York")
    let e = try #require(mapper().event(source, sourceID: "s"))
    #expect(e.isAllDay && e.start == source.start && e.end == source.end)
}

@Test func allDayFromAnotherZoneLandsOnTheSameLocalDates() throws {
    // Tokyo 2026-09-18 viewed on a New York device: local midnight of the 18th to local midnight of the 19th.
    let e = try #require(mapper().event(allDay(first: (2026, 9, 18), endExclusive: (2026, 9, 19), zone: "Asia/Tokyo"), sourceID: "s"))
    #expect(e.start == iso("2026-09-18T04:00:00Z") && e.end == iso("2026-09-19T04:00:00Z"))
    // And the reverse: a New York all-day event on a Tokyo device.
    let t = try #require(mapper(in: "Asia/Tokyo").event(allDay(first: (2026, 9, 18), endExclusive: (2026, 9, 19), zone: "America/New_York"), sourceID: "s"))
    #expect(t.start == iso("2026-09-17T15:00:00Z") && t.end == iso("2026-09-18T15:00:00Z"))
}

@Test func multiDayAllDayKeepsItsLength() throws {
    let e = try #require(mapper().event(allDay(first: (2026, 9, 18), endExclusive: (2026, 9, 21), zone: "Asia/Tokyo"), sourceID: "s"))
    #expect(e.start == iso("2026-09-18T04:00:00Z") && e.end == iso("2026-09-21T04:00:00Z"))
}

@Test func mapsCalendarDescriptors() {
    let d = CalendarDescriptor(id: "c", title: "Work", colorHex: "#abc", accountName: "me@x.test")
    let info = mapper().calendarInfo(d, sourceID: "google-1")
    #expect(info == CalendarInfo(sourceID: "google-1", calendarID: "c", title: "Work", accountName: "me@x.test", colorHex: "#AABBCC"))
}

@Test func allDayOnAFallBackDayCoversTheLongLocalDay() throws {
    // New York falls back on 2026-11-01: local midnight is EDT (04:00Z) and the next is EST (05:00Z), a 25-hour day.
    let source = allDay(first: (2026, 11, 1), endExclusive: (2026, 11, 2), zone: "Asia/Tokyo")
    #expect(source.start == iso("2026-10-31T15:00:00Z") && source.end == iso("2026-11-01T15:00:00Z"))
    let e = try #require(mapper().event(source, sourceID: "s"))
    #expect(e.isAllDay)
    #expect(e.start == iso("2026-11-01T04:00:00Z") && e.end == iso("2026-11-02T05:00:00Z"))
}

@Test func eventKitAndGoogleShapedEventsAgreeOnSharedKeysAndDedupAsExact() throws {
    let me = CalendarCore.Attendee(name: "Me", email: "me@x.test", response: .accepted, isSelf: true)
    let other = CalendarCore.Attendee(name: "Bo", email: "bo@x.test")
    func library(_ configure: (inout CalendarCore.CalendarEvent) -> Void) -> CalendarCore.CalendarEvent {
        var e = CalendarCore.CalendarEvent(
            eventID: "id", uid: "UID-1", calendarID: "cal", title: "Design Review",
            start: iso("2026-09-18T14:00:00Z"), end: iso("2026-09-18T14:30:00Z"))
        configure(&e)
        return e
    }
    let eventKit = library { $0.attendees = [me, other] }
    let google = library { $0.attendees = [other]; $0.myResponse = .accepted }
    let a = try #require(mapper().event(eventKit, sourceID: "eventkit"))
    let b = try #require(mapper().event(google, sourceID: "google-1"))
    #expect(a.externalUID == "UID-1" && a.externalUID == b.externalUID)
    #expect(a.title == b.title && a.start == b.start && a.end == b.end)
    #expect(a.contentKey == b.contentKey)
    #expect(a.sourceID != b.sourceID)
    #expect(DuplicateRules.decide(a, b) == .merge(.exactMatch))
}
