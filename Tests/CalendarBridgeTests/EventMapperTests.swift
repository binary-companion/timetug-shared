import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import CalendarBridge

private func zone(_ id: String) -> TimeZone { TimeZone(identifier: id)! }
private func iso(_ s: String) -> Date { ISO8601DateFormatter().date(from: s)! }
private func mapper() -> EventMapper { EventMapper() }
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

@Test func wrapsTimedEventUnchanged() throws {
    let source = timed { $0.conference = ConferenceInfo(url: URL(string: "https://meet.example/x")!, provider: .meet) }
    let mapped = try #require(mapper().event(source, sourceID: "google-1"))
    #expect(mapped.event == source)
    #expect(mapped.sourceID == "google-1")
}

@Test func allDayEventsPassThroughUnchanged() throws {
    let source = allDay(first: (2026, 9, 21), endExclusive: (2026, 9, 22), zone: "Asia/Tokyo")
    let mapped = try #require(mapper().event(source, sourceID: "google-1"))
    #expect(mapped.event == source)
    #expect(mapped.event.start == source.start && mapped.event.end == source.end)
    #expect(mapped.sourceID == "google-1")
}

@Test func cancelledEventsAreDropped() {
    #expect(mapper().event(timed { $0.status = .cancelled }, sourceID: "s") == nil)
}

@Test func titlesPassThroughUnchangedIncludingEmpty() throws {
    #expect(try #require(mapper().event(timed { $0.title = "" }, sourceID: "s")).title == "")
}

@Test func mapsCalendarDescriptors() {
    let d = CalendarDescriptor(id: "c", title: "Work", colorHex: "#abc", accountName: "me@x.test")
    let info = mapper().calendarInfo(d, sourceID: "google-1")
    #expect(info == CalendarInfo(sourceID: "google-1", calendarID: "c", title: "Work", accountName: "me@x.test", colorHex: "#AABBCC"))
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
