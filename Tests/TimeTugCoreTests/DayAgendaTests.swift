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
