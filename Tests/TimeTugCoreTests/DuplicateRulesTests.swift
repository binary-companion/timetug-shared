import Foundation
import Testing
@testable import TimeTugCore

private func decide(_ a: CalendarEvent, _ b: CalendarEvent) -> PairDecision { DuplicateRules.decide(a, b) }

@Test func exactMatchMergesEvenOnOneCalendar() {
    #expect(decide(makeEvent("1", title: "Sync"), makeEvent("2", title: "sync", calendarID: "other")) == .merge(.exactMatch))
    #expect(decide(makeEvent("1", title: "Sync"), makeEvent("2", title: "Sync")) == .merge(.exactMatch))
}

@Test func sameCalendarDifferentTitlesStaySeparate() {
    #expect(decide(makeEvent("1", title: "Doctor"), makeEvent("2", title: "Dance")) == .separate(.sameCalendar))
}

@Test func allDayEventsOnlyMergeOnExactMatch() {
    let a = makeEvent("1", title: "Birthday", isAllDay: true)
    let b = makeEvent("2", title: "Kristin birthday", calendarID: "other", isAllDay: true)
    #expect(decide(a, b) == .separate(.allDay))
}

@Test func timeGateBoundaries() {
    let a = makeEvent("1", title: "A", minutes: 60)                                        // 10:00-11:00
    #expect(decide(a, makeEvent("2", title: "B", start: "2026-09-18T10:30:00Z", minutes: 60, calendarID: "o")) == .ambiguous)
    #expect(decide(a, makeEvent("2", title: "B", start: "2026-09-18T10:31:00Z", minutes: 60, calendarID: "o")) == .separate(.outsideTimeGate))
    #expect(decide(makeEvent("1", title: "A", minutes: 30), makeEvent("2", title: "B", minutes: 90, calendarID: "o")) == .ambiguous)      // end +60
    #expect(decide(makeEvent("1", title: "A", minutes: 30), makeEvent("2", title: "B", minutes: 91, calendarID: "o")) == .separate(.outsideTimeGate))
}

@Test func sharedConferenceMergesDespiteDifferentTitlesAndQueries() {
    let a = makeEvent("1", title: "Weekly", location: "https://acme.zoom.us/j/555?pwd=abc")
    let b = makeEvent("2", title: "Team sync", calendarID: "o", notes: "Join https://acme.zoom.us/j/555/")
    #expect(decide(a, b) == .merge(.conferenceLink))
}

@Test func sharedExternalUIDMerges() {
    let a = makeEvent("1", title: "A", externalUID: "uid-1")
    let b = makeEvent("2", title: "B", calendarID: "o", externalUID: "uid-1")
    #expect(decide(a, b) == .merge(.externalUID))
}

@Test func sharedAttendeeEmailMergesCaseInsensitively() {
    let a = makeEvent("1", title: "A", attendees: [Attendee(email: "Kristin@x.com")])
    let b = makeEvent("2", title: "B", calendarID: "o", attendees: [Attendee(email: "kristin@X.com"), Attendee(email: "z@x.com")])
    #expect(decide(a, b) == .merge(.sharedAttendee))
}

@Test func sameLocationMergesIncludingContainment() {
    let a = makeEvent("1", title: "Doctor", location: "1234 Main St")
    let b = makeEvent("2", title: "Intermountain Health", calendarID: "o", location: "Intermountain Health, 1234 Main St")
    #expect(decide(a, b) == .merge(.sameLocation))
}

@Test func vetoesFireOnlyWhenBothSidesHaveTheField() {
    let loc1 = makeEvent("1", title: "A", location: "Room 101 East")
    let loc2 = makeEvent("2", title: "B", calendarID: "o", location: "Dentist Office")
    #expect(decide(loc1, loc2) == .separate(.conflictingLocation))

    let z1 = makeEvent("1", title: "A", conferenceURL: URL(string: "https://acme.zoom.us/j/1"))
    let z2 = makeEvent("2", title: "B", calendarID: "o", conferenceURL: URL(string: "https://acme.zoom.us/j/2"))
    #expect(decide(z1, z2) == .separate(.conflictingConference))

    let p1 = makeEvent("1", title: "A", attendees: [Attendee(email: "a@x.com")])
    let p2 = makeEvent("2", title: "B", calendarID: "o", attendees: [Attendee(email: "b@x.com")])
    #expect(decide(p1, p2) == .separate(.conflictingAttendees))

    // Absent on one side is not a veto.
    #expect(decide(loc1, makeEvent("2", title: "B", calendarID: "o")) == .ambiguous)
    #expect(decide(makeEvent("1", title: "Scott: Doctor"), makeEvent("2", title: "Intermountain Health", calendarID: "o")) == .ambiguous)
}

@Test func detailScoreAndSummary() {
    #expect(DuplicateRules.detailSummary(makeEvent(others: 0)) == "bare")
    let rich = makeEvent(others: 2, location: "1 Main St", notes: "Bring card")
    #expect(DuplicateRules.detailSummary(rich) == "location+attendees+notes")
    #expect(DuplicateRules.detailScore(rich) == 3)
}

@Test func participantsAreTheEventItselfWhenNeverMerged() {
    let event = makeEvent("1", title: "Doctor")
    #expect(event.participants.map(\.title) == ["Doctor"])
    #expect(event.participants.first?.calendarKey == "fake/cal")
}

@Test func webexLinksAreIdentifiedByTheirMeetingIDInTheQuery() {
    let a = makeEvent("1", title: "Weekly", calendarID: "a", conferenceURL: URL(string: "https://acme.webex.com/acme/j.php?MTID=m111")!)
    let sameMeeting = makeEvent("2", title: "Team sync", calendarID: "b", conferenceURL: URL(string: "https://acme.webex.com/acme/j.php?MTID=M111&x=1")!)
    let other = makeEvent("3", title: "Team sync", calendarID: "b", conferenceURL: URL(string: "https://acme.webex.com/acme/j.php?MTID=m222")!)
    #expect(decide(a, sameMeeting) == .merge(.conferenceLink))
    #expect(decide(a, other) != .merge(.conferenceLink))
}

@Test func aPlainWebsiteInTheURLFieldIsNotAConferenceIdentity() {
    let page = URL(string: "https://example.com/event/42")!
    let a = makeEvent("1", title: "Weekly", calendarID: "a", url: page)
    let b = makeEvent("2", title: "Team sync", calendarID: "b", url: page)
    #expect(DuplicateRules.conferenceIdentity(a) == nil)
    #expect(decide(a, b) != .merge(.conferenceLink))
    let structured = makeEvent("3", title: "Weekly", calendarID: "a", conferenceURL: page)
    #expect(DuplicateRules.conferenceIdentity(structured) == nil)
}
