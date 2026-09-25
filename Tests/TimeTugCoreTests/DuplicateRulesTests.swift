import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private func decide(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> PairDecision { DuplicateRules.decide(a, b) }

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
    let a = makeEvent("1", title: "A", attendees: [CalendarCore.Attendee(email: "Kristin@x.com")])
    let b = makeEvent("2", title: "B", calendarID: "o", attendees: [CalendarCore.Attendee(email: "kristin@X.com"), CalendarCore.Attendee(email: "z@x.com")])
    #expect(decide(a, b) == .merge(.sharedAttendee))
}

@Test func sharedAttendeeDoesNotOverrideAConflictingLocation() {
    let a = makeEvent("1", title: "A", location: "Room 101 East", attendees: [CalendarCore.Attendee(email: "k@x.com")])
    let b = makeEvent("2", title: "B", calendarID: "o", location: "Dentist Office", attendees: [CalendarCore.Attendee(email: "k@x.com")])
    #expect(decide(a, b) == .separate(.conflictingLocation))
}

@Test func sharedAttendeeDoesNotOverrideDifferentConferenceLinks() {
    let a = makeEvent("1", title: "A", conferenceURL: URL(string: "https://acme.zoom.us/j/1"), attendees: [CalendarCore.Attendee(email: "k@x.com")])
    let b = makeEvent("2", title: "B", calendarID: "o", conferenceURL: URL(string: "https://acme.zoom.us/j/2"), attendees: [CalendarCore.Attendee(email: "k@x.com")])
    #expect(decide(a, b) == .separate(.conflictingConference))
}

@Test func sharedOrganizerAloneMergesWhenNothingConflicts() {
    var a = makeEvent("1", title: "A")
    a.event.organizer = CalendarCore.Attendee(email: "Boss@x.com", isOrganizer: true)
    var b = makeEvent("2", title: "B", calendarID: "o")
    b.event.organizer = CalendarCore.Attendee(email: "boss@x.com", isOrganizer: true)
    #expect(decide(a, b) == .merge(.sharedAttendee))
}

@Test func sharedAttendeeWithSameLocationReportsSharedAttendee() {
    // Shared attendee is checked before same location, so it supplies the reason.
    let a = makeEvent("1", title: "A", location: "Room 101 East", attendees: [CalendarCore.Attendee(email: "k@x.com")])
    let b = makeEvent("2", title: "B", calendarID: "o", location: "Room 101 East", attendees: [CalendarCore.Attendee(email: "k@x.com")])
    #expect(decide(a, b) == .merge(.sharedAttendee))
}

@Test func sameLocationDoesNotOverrideDifferentConferenceLinks() {
    let a = makeEvent("1", title: "A", location: "Room 101 East", conferenceURL: URL(string: "https://acme.zoom.us/j/1"))
    let b = makeEvent("2", title: "B", calendarID: "o", location: "Room 101 East", conferenceURL: URL(string: "https://acme.zoom.us/j/2"))
    #expect(decide(a, b) == .separate(.conflictingConference))
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

    let p1 = makeEvent("1", title: "A", attendees: [CalendarCore.Attendee(email: "a@x.com")])
    let p2 = makeEvent("2", title: "B", calendarID: "o", attendees: [CalendarCore.Attendee(email: "b@x.com")])
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
    #expect(DuplicateRules.conferenceIdentities(a).isEmpty)
    #expect(decide(a, b) != .merge(.conferenceLink))
}

@Test func unlistedProvidersStillVetoWhenTheSourceSuppliesTheLink() {
    let mail = [CalendarCore.Attendee(email: "k@x.com")]
    let a = makeEvent("1", title: "A", conferenceURL: URL(string: "https://chime.aws/111"), attendees: mail)
    let b = makeEvent("2", title: "B", calendarID: "o", conferenceURL: URL(string: "https://chime.aws/222"), attendees: mail)
    #expect(decide(a, b) == .separate(.conflictingConference))
    let c = makeEvent("3", title: "C", calendarID: "o", conferenceURL: URL(string: "https://chime.aws/111"), attendees: mail)
    #expect(decide(a, c) == .merge(.conferenceLink))
}

@Test func aDetectedGenericURLIsNotAConferenceIdentity() {
    let a = makeEvent("1", title: "A", url: URL(string: "https://example.com/x"))
    #expect(DuplicateRules.conferenceIdentities(a).isEmpty)
}

@Test func sameTitleInsideTheTimeGateMergesEvenWhenTheEndsDiffer() {
    let a = makeEvent("1", title: "Dance Party", start: "2026-09-18T19:15:00Z", minutes: 45, calendarID: "shared")
    let b = makeEvent("2", title: "Dance Party", start: "2026-09-18T19:15:00Z", minutes: 60, calendarID: "blackout")
    #expect(decide(a, b) == .merge(.sameTitle))
    let later = makeEvent("3", title: "dance  party!", start: "2026-09-18T19:30:00Z", minutes: 45, calendarID: "blackout")
    #expect(decide(a, later) == .merge(.sameTitle))   // punctuation, case and spacing do not matter; a 15 min start gap is inside the gate
}

@Test func sameTitleStillNeedsTheTimeGateAndADifferentCalendar() {
    let a = makeEvent("1", title: "Dance Party", minutes: 60)
    #expect(decide(a, makeEvent("2", title: "Dance Party", start: "2026-09-18T10:31:00Z", minutes: 60, calendarID: "o")) == .separate(.outsideTimeGate))
    #expect(decide(a, makeEvent("3", title: "Dance Party", minutes: 90)) == .separate(.sameCalendar))
}

@Test func sameTitleDoesNotOverrideAConflictingPlaceOrPeople() {
    let a = makeEvent("1", title: "Standup", location: "Room A", attendees: [CalendarCore.Attendee(email: "x@acme.com")])
    let otherRoom = makeEvent("2", title: "Standup", minutes: 45, calendarID: "o", location: "Room B")
    #expect(decide(a, otherRoom) == .separate(.conflictingLocation))
    let otherPeople = makeEvent("3", title: "Standup", minutes: 45, calendarID: "o", attendees: [CalendarCore.Attendee(email: "y@acme.com")])
    #expect(decide(a, otherPeople) == .separate(.conflictingAttendees))
}

// UID scope (Issue 9).

@Test func uidMatchKeyDependsOnTheScope() {
    #expect(makeEvent("1", externalUID: "u", uidScope: .global).uidMatchKey == "u")
    #expect(makeEvent("1", externalUID: "u", uidScope: .provider, service: "eventkit", provider: "microsoft").uidMatchKey == "eventkit|microsoft|u")
    #expect(makeEvent("1", externalUID: "u", uidScope: nil, service: "eventkit").uidMatchKey == "eventkit|?|u")
    #expect(makeEvent("1", externalUID: "u").uidMatchKey == "?|?|u")
    #expect(makeEvent("1").uidMatchKey == nil)
}

@Test func exchangeCopiesInsideOneStoreStillMergeOnTheirID() {
    let a = makeEvent("1", title: "A", externalUID: "exch-1", uidScope: .provider, service: "eventkit", provider: "microsoft")
    let b = makeEvent("2", title: "B", calendarID: "o", externalUID: "exch-1", uidScope: .provider, service: "eventkit", provider: "microsoft")
    #expect(decide(a, b) == .merge(.externalUID))
}

@Test func anExchangeIDNeverMatchesAnotherServicesUID() {
    let exchange = makeEvent("1", title: "A", externalUID: "same", uidScope: .provider, service: "eventkit", provider: "microsoft")
    let google = makeEvent("2", title: "B", calendarID: "o", externalUID: "same", uidScope: .global, service: "google", provider: "google")
    #expect(decide(exchange, google) != .merge(.externalUID))
}

@Test func globalUIDsMatchAcrossServices() {
    let a = makeEvent("1", title: "A", externalUID: "same", uidScope: .global, service: "eventkit", provider: "icloud")
    let b = makeEvent("2", title: "B", calendarID: "o", externalUID: "same", uidScope: .global, service: "google", provider: "google")
    #expect(decide(a, b) == .merge(.externalUID))
}
