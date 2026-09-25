import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private func library(
    _ id: String = "e1", start: String = "2026-09-18T10:00:00Z", minutes: Int = 30,
    attendees: [CalendarCore.Attendee] = [], organizer: CalendarCore.Attendee? = nil,
    myResponse: CalendarCore.ResponseStatus? = nil, conference: URL? = nil
) -> CalendarCore.CalendarEvent {
    let s = date(start)
    return CalendarCore.CalendarEvent(
        eventID: id, uid: "uid-1", calendarID: "cal", title: "Standup", start: s,
        end: s.addingTimeInterval(TimeInterval(minutes * 60)), attendees: attendees, organizer: organizer,
        conferences: conference.map { [ConferenceInfo(url: $0, provider: .other)] } ?? [], myResponse: myResponse)
}

@Test func forwardsAndSetsTheEventsFields() {
    var e = TimeTugCalendarEvent(event: library(), sourceID: "src")
    #expect(e.title == "Standup" && e.calendarID == "cal" && e.sourceEventID == "e1" && e.externalUID == "uid-1")
    e.start = date("2026-09-18T11:00:00Z")
    e.location = "Room 4"
    #expect(e.event.start == date("2026-09-18T11:00:00Z") && e.event.location == "Room 4")
}

@Test func derivesAttendeeCountResponseAndOrganizerFromTheEvent() {
    let me = CalendarCore.Attendee(email: "me@x.com", response: .tentative, isSelf: true)
    let other = CalendarCore.Attendee(email: "A@X.com", response: .accepted)
    let boss = CalendarCore.Attendee(email: "boss@x.com", isOrganizer: true)
    let e = TimeTugCalendarEvent(event: library(attendees: [me, other], organizer: boss), sourceID: "s")
    #expect(e.otherAttendeeCount == 1)
    #expect(e.attendees.map(\.email) == ["a@x.com"])
    #expect(e.responseStatus == .tentative)
    #expect(e.organizerEmail == "boss@x.com")
}

@Test func myResponseWinsAndSelfOrganizerIsHidden() {
    let me = CalendarCore.Attendee(email: "me@x.com", response: .needsAction, isSelf: true, isOrganizer: true)
    let e = TimeTugCalendarEvent(event: library(attendees: [me], organizer: me, myResponse: .accepted), sourceID: "s")
    #expect(e.responseStatus == .accepted)
    #expect(e.organizerEmail == nil)
    #expect(TimeTugCalendarEvent(event: library(), sourceID: "s").responseStatus == nil)
}

@Test func conferencesStartFromTheEventAndTheFirstIsTheJoinURL() {
    let url = URL(string: "https://meet.google.com/aaa-bbbb-ccc")!
    var e = TimeTugCalendarEvent(event: library(conference: url), sourceID: "s")
    #expect(e.conferenceURL == url)
    e.conferences = []
    #expect(e.conferenceURL == nil)
    #expect(e.event.conferences.first?.url == url)
}

@Test func isSameMeetingMatchesByIdOrByAnyMergedContentKey() {
    let a = TimeTugCalendarEvent(event: library("e1"), sourceID: "src")
    let sameOccurrence = TimeTugCalendarEvent(event: library("e1"), sourceID: "src")
    let sameContentOtherID = TimeTugCalendarEvent(event: library("e9"), sourceID: "other")
    var merged = TimeTugCalendarEvent(event: library("m", start: "2026-09-18T12:00:00Z"), sourceID: "src")
    merged.mergedMembers = [MergedMember(title: "Standup", calendarKey: "src/cal", contentKey: a.contentKey,
                                         details: "bare", start: a.start, end: a.end)]
    #expect(a.isSameMeeting(as: sameOccurrence))
    #expect(a.isSameMeeting(as: sameContentOtherID))   // same title, start and end
    #expect(merged.isSameMeeting(as: a))                // a merged copy's content matches
    #expect(!a.isSameMeeting(as: TimeTugCalendarEvent(event: library("e2", start: "2026-09-18T15:00:00Z"), sourceID: "src")))
}

@Test func idAndContentKeyKeepTheirFormulas() {
    let e = TimeTugCalendarEvent(event: library(), sourceID: "src")
    let start = Int(date("2026-09-18T10:00:00Z").timeIntervalSince1970)
    let end = Int(date("2026-09-18T10:30:00Z").timeIntervalSince1970)
    #expect(e.id == "src/e1/\(start)")
    #expect(e.contentKey == "standup|\(start)|\(end)")
    #expect(e.calendarKey == "src/cal")
}
