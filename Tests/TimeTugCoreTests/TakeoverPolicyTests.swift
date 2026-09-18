import Foundation
import Testing
@testable import TimeTugCore

@Test func qualifiesWhenOptedInAndDefaultsPass() {
    #expect(TakeoverPolicy.qualifies(makeEvent(), settings: optedIn()))
}

@Test func rejectsCalendarNotOptedIn() {
    #expect(!TakeoverPolicy.qualifies(makeEvent(calendarID: "family"), settings: optedIn()))
}

@Test func rejectsAllDay() {
    #expect(!TakeoverPolicy.qualifies(makeEvent(isAllDay: true), settings: optedIn()))
}

@Test func rejectsDeclinedByDefaultButAllowsWhenToggledOff() {
    let declined = makeEvent(status: .declined)
    #expect(!TakeoverPolicy.qualifies(declined, settings: optedIn()))
    #expect(TakeoverPolicy.qualifies(declined, settings: optedIn { $0.skipDeclinedEvents = false }))
}

@Test func rejectsSoloByDefaultButAllowsWhenToggledOff() {
    let solo = makeEvent(others: 0)
    #expect(!TakeoverPolicy.qualifies(solo, settings: optedIn()))
    #expect(TakeoverPolicy.qualifies(solo, settings: optedIn { $0.skipSoloEvents = false }))
}

@Test func requireConferenceLinkFiltersEventsWithoutOne() {
    let settings = optedIn { $0.requireConferenceLink = true }
    #expect(!TakeoverPolicy.qualifies(makeEvent(), settings: settings))
    let withLink = makeEvent(conferenceURL: URL(string: "https://meet.google.com/a-b-c"))
    #expect(TakeoverPolicy.qualifies(withLink, settings: settings))
}
