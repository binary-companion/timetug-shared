import Foundation
import Testing
@testable import TimeTugCore

@Test func qualifiesWhenOptedInAndDefaultsPass() {
    #expect(TakeoverPolicy.qualifies(makeEvent(), settings: optedIn()))
}

@Test func rejectsCalendarNotOptedIn() {
    #expect(!TakeoverPolicy.qualifies(makeEvent(calendarID: "family"), settings: optedIn()))
}

@Test func rejectsAllDayByDefault() {
    #expect(!TakeoverPolicy.qualifies(makeEvent(isAllDay: true), settings: optedIn()))
}

@Test func allDayNeverTakesOverEvenWhenSkipAllDayIsOff() {
    let allDay = makeEvent(isAllDay: true)
    #expect(!TakeoverPolicy.qualifies(allDay, settings: optedIn { $0.skipAllDayEvents = false }))
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

@Test func qualifiesWhenDuplicateOnAnOptedInCalendar() {
    var event = makeEvent(calendarID: "family")
    event.additionalCalendarKeys = ["fake/cal"]
    #expect(TakeoverPolicy.qualifies(event, settings: optedIn()))
    event.additionalCalendarKeys = []
    #expect(!TakeoverPolicy.qualifies(event, settings: optedIn()))
}

@Test func disabledSettingBlocksAnOtherwiseQualifyingEvent() {
    #expect(TakeoverPolicy.qualifies(makeEvent(), settings: optedIn()))
    #expect(!TakeoverPolicy.qualifies(makeEvent(), settings: optedIn { $0.disabled = true }))
}
