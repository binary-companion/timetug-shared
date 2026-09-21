import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

@Test func allDayDatesAreReadInTheEventsOwnZone() {
    let e = makeAllDay(zone: "Asia/Tokyo", first: day(2026, 9, 21), endExclusive: day(2026, 9, 22))
    #expect(e.allDayDates?.first == day(2026, 9, 21))
    #expect(e.covers(day(2026, 9, 21)))
    #expect(!e.covers(day(2026, 9, 22)))
    #expect(!e.covers(day(2026, 9, 20)))
}

@Test func multiDayAllDayCoversEveryDayButNotTheExclusiveEnd() {
    let e = makeAllDay(zone: "America/Los_Angeles", first: day(2026, 9, 21), endExclusive: day(2026, 9, 24))
    #expect(e.covers(day(2026, 9, 23)))
    #expect(!e.covers(day(2026, 9, 24)))
}

@Test func allDayDatesSurviveAMissingMidnight() {
    // Sao Paulo skipped local midnight on 2018-11-04 (DST began at 00:00); the noon-based start of day still works.
    let e = makeAllDay(zone: "America/Sao_Paulo", first: day(2018, 11, 4), endExclusive: day(2018, 11, 5))
    #expect(e.allDayDates?.first == day(2018, 11, 4))
    #expect(e.allDayDates?.endExclusive == day(2018, 11, 5))
    #expect(e.covers(day(2018, 11, 4)))
}

@Test func timedEventsHaveNoAllDayDates() {
    let e = makeEvent()
    #expect(e.allDayDates == nil)
    #expect(!e.covers(day(2026, 9, 18)))
}

@Test func allDayRangeWithinOneDateIsTreatedAsThatDay() {
    // Non-canonical input (start and end on the same date in the event's zone): must not vanish.
    let e = makeEvent("x", start: "2026-09-18T10:00:00Z", minutes: 30, isAllDay: true)
    #expect(e.allDayDates?.first == day(2026, 9, 18))
    #expect(e.allDayDates?.endExclusive == day(2026, 9, 19))
    #expect(e.covers(day(2026, 9, 18)))
    #expect(!e.covers(day(2026, 9, 19)))
}
