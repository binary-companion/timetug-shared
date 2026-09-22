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

@Test func alwaysRejectsDeclinedEvents() {
    let declined = makeEvent(status: .declined)
    #expect(!TakeoverPolicy.qualifies(declined, settings: optedIn()))
    #expect(!TakeoverPolicy.qualifies(declined, settings: optedIn { $0.requireConferenceLink = true; $0.requireOtherAttendees = true }))
}

@Test func soloEventsQualifyByDefaultAndAreRejectedWhenOtherAttendeesRequired() {
    let solo = makeEvent(others: 0)
    #expect(TakeoverPolicy.qualifies(solo, settings: optedIn()))
    #expect(!TakeoverPolicy.qualifies(solo, settings: optedIn { $0.requireOtherAttendees = true }))
    #expect(TakeoverPolicy.qualifies(makeEvent(others: 1), settings: optedIn { $0.requireOtherAttendees = true }))
}

@Test func requireConferenceLinkFiltersEventsWithoutOne() {
    let settings = optedIn { $0.requireConferenceLink = true }
    #expect(!TakeoverPolicy.qualifies(makeEvent(), settings: settings))
    let withLink = makeEvent(conferenceURL: URL(string: "https://meet.google.com/a-b-c"))
    #expect(TakeoverPolicy.qualifies(withLink, settings: settings))
}

@Test func aHiddenCalendarNeverTugsEvenIfOptedIn() {
    let hidden = optedIn { $0.hiddenCalendarKeys = ["fake/cal"] }
    #expect(!TakeoverPolicy.qualifies(makeEvent(), settings: hidden))
}

@Test func aMergedEventNeedsAnOptedInCopyThatIsShown() {
    var event = makeEvent(calendarID: "family")
    event.additionalCalendarKeys = ["fake/cal"]
    #expect(TakeoverPolicy.qualifies(event, settings: optedIn()))
    #expect(!TakeoverPolicy.qualifies(event, settings: optedIn { $0.hiddenCalendarKeys = ["fake/cal"] }))
}

@Test func qualifiesWhenDuplicateOnAnOptedInCalendar() {
    var event = makeEvent(calendarID: "family")
    event.additionalCalendarKeys = ["fake/cal"]
    #expect(TakeoverPolicy.qualifies(event, settings: optedIn()))
    event.additionalCalendarKeys = []
    #expect(!TakeoverPolicy.qualifies(event, settings: optedIn()))
}

@Test func turningTugOffBlocksAnOtherwiseQualifyingEvent() {
    #expect(TakeoverPolicy.qualifies(makeEvent(), settings: optedIn()))
    #expect(!TakeoverPolicy.qualifies(makeEvent(), settings: optedIn { $0.enabled = false }))
}
