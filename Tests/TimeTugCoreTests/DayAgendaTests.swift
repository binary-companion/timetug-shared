import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T10:15:00Z")

private func agenda(_ events: [TimeTugCalendarEvent], settings: TakeoverSettings = optedIn(),
                    at time: Date = now) -> DayAgenda {
    DayAgenda.make(events: events, settings: settings, now: time, calendar: utcCalendar)
}

@Test func marksPastCurrentUpcoming() {
    let past = makeEvent("p", start: "2026-09-18T09:00:00Z")       // ends 09:30
    let current = makeEvent("c", start: "2026-09-18T10:00:00Z")    // ends 10:30
    let upcoming = makeEvent("u", start: "2026-09-18T11:00:00Z")
    let result = agenda([past, current, upcoming])
    #expect(result.items.map(\.state) == [.past, .current, .upcoming])
}

@Test func nextIsFirstTimedEventStartingAfterNow() {
    let current = makeEvent("c", start: "2026-09-18T10:00:00Z")
    let allDay = makeEvent("a", start: "2026-09-18T00:00:00Z", minutes: 1440, isAllDay: true)
    let upcoming = makeEvent("u", start: "2026-09-18T11:00:00Z")
    let settings = optedIn { $0.skipAllDayEvents = false }
    #expect(agenda([allDay, current, upcoming], settings: settings).next == upcoming)
}

@Test func nextIsNilWhenNothingRemains() {
    #expect(agenda([makeEvent(start: "2026-09-18T09:00:00Z")]).next == nil)
}

@Test func hiddenCalendarsAreExcluded() {
    let settings = optedIn { $0.hiddenCalendarKeys = ["fake/family"] }
    let result = agenda([makeEvent("a", calendarID: "family"), makeEvent("b")], settings: settings)
    #expect(result.items.map(\.event.sourceEventID) == ["b"])
}

@Test func mergedMeetingIsHiddenOnlyWhenAllItsCalendarsAreHidden() {
    var merged = makeEvent("m", calendarID: "family")
    merged.additionalCalendarKeys = ["fake/work"]
    let hideFamilyOnly = optedIn { $0.hiddenCalendarKeys = ["fake/family"] }
    let hideBoth = optedIn { $0.hiddenCalendarKeys = ["fake/family", "fake/work"] }
    #expect(agenda([merged], settings: hideFamilyOnly).items.count == 1)
    #expect(agenda([merged], settings: hideBoth).items.isEmpty)
}

@Test func includesEventsThatOverlapToday() {
    let overnight = makeEvent("n", start: "2026-09-17T23:30:00Z", minutes: 120)
    #expect(agenda([overnight]).items.count == 1)
}

@Test func excludesYesterdayAndFarTomorrow() {
    let yesterday = makeEvent("y", start: "2026-09-17T10:00:00Z")
    let tomorrow = makeEvent("t", start: "2026-09-19T10:00:00Z")
    #expect(agenda([yesterday, tomorrow]).items.isEmpty)
}

@Test func afterMidnightEventAppearsOnlyInsideLeadTime() {
    let event = makeEvent("m", start: "2026-09-19T00:05:00Z")
    let settings = optedIn { $0.leadTime = 600 }
    #expect(agenda([event], settings: settings, at: date("2026-09-18T23:40:00Z")).items.isEmpty)
    #expect(agenda([event], settings: settings, at: date("2026-09-18T23:56:00Z")).items.count == 1)
}

@Test func tomorrowsAllDayEventNeverSpillsIn() {
    let allDay = makeEvent("a", start: "2026-09-19T00:00:00Z", minutes: 1440, isAllDay: true)
    let settings = optedIn { $0.leadTime = 600; $0.skipAllDayEvents = false }
    #expect(agenda([allDay], settings: settings, at: date("2026-09-18T23:59:00Z")).items.isEmpty)
}

@Test func allDayEventIsHiddenByDefault() {
    let allDay = makeEvent("a", start: "2026-09-18T00:00:00Z", minutes: 1440, isAllDay: true)
    #expect(agenda([allDay]).items.isEmpty)
}

@Test func allDayEventIsShownWhenNotSkipped() {
    let allDay = makeEvent("a", start: "2026-09-18T00:00:00Z", minutes: 1440, isAllDay: true)
    let settings = optedIn { $0.skipAllDayEvents = false }
    #expect(agenda([allDay], settings: settings).items.map(\.event.sourceEventID) == ["a"])
}

@Test func timedEventIsUnaffectedBySkipAllDaySetting() {
    let timed = makeEvent("t", start: "2026-09-18T11:00:00Z")
    let skip = optedIn { $0.skipAllDayEvents = true }
    let keep = optedIn { $0.skipAllDayEvents = false }
    #expect(agenda([timed], settings: skip).items.count == 1)
    #expect(agenda([timed], settings: keep).items.count == 1)
}

private let la = calendar(in: "America/Los_Angeles")

// The first test below fails against the old instant-based rule; the second and third are regression guards that
// pin behaviour that must not change (exclusive end date, skipAllDayEvents).

private func laAgenda(_ events: [TimeTugCalendarEvent], at time: String) -> DayAgenda {
    DayAgenda.make(events: events, settings: optedIn { $0.skipAllDayEvents = false }, now: date(time), calendar: la)
}

@Test func allDayEventStaysOnItsOwnDateForAViewerInAnotherZone() {
    let tokyoSep21 = makeAllDay(zone: "Asia/Tokyo", first: day(2026, 9, 21), endExclusive: day(2026, 9, 22))
    // 10:00Z on Sep 21 is 03:00 in Los Angeles: still Sep 21 there.
    let onTheDay = laAgenda([tokyoSep21], at: "2026-09-21T10:00:00Z")
    #expect(onTheDay.items.map(\.state) == [.current])
    // 10:00Z on Sep 20 is Sep 20 in Los Angeles: not shown (the Tokyo event's instants begin at 15:00Z on Sep 20,
    // which the old instant rule would have shown as today's).
    #expect(laAgenda([tokyoSep21], at: "2026-09-20T10:00:00Z").items.isEmpty)
}

@Test func allDayEventEndingAtLocalMidnightIsNotShownOnTheExclusiveEndDate() {
    let twoDays = makeAllDay(zone: "America/Los_Angeles", first: day(2026, 9, 19), endExclusive: day(2026, 9, 21))
    #expect(laAgenda([twoDays], at: "2026-09-20T18:00:00Z").items.count == 1)   // Sep 20 in LA
    #expect(laAgenda([twoDays], at: "2026-09-21T18:00:00Z").items.isEmpty)      // Sep 21 in LA
}

@Test func allDayItemsAreHiddenWhenSkipAllDayIsOn() {
    let holiday = makeAllDay(zone: "America/Los_Angeles", first: day(2026, 9, 21), endExclusive: day(2026, 9, 22))
    let result = DayAgenda.make(events: [holiday], settings: optedIn(), now: date("2026-09-21T18:00:00Z"), calendar: la)
    #expect(result.items.isEmpty)
}
