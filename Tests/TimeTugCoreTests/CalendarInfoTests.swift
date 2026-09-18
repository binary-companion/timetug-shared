import Testing
import Foundation
@testable import TimeTugCore

@Test func calendarInfoAccountNameDefaultsToNil() {
    #expect(CalendarInfo(sourceID: "s", calendarID: "c", title: "T").accountName == nil)
}

@Test func calendarInfoKeyIgnoresAccountName() {
    let a = CalendarInfo(sourceID: "s", calendarID: "c", title: "T", accountName: "iCloud")
    let b = CalendarInfo(sourceID: "s", calendarID: "c", title: "T", accountName: "Google")
    #expect(a != b)
    #expect(a.key == b.key)
    #expect(a.key == "s/c")
}

@Test func refreshPreservesCalendarAccountName() async {
    let source = FakeSource()
    await source.set(calendars: [CalendarInfo(sourceID: "fake", calendarID: "c1", title: "Shared", accountName: "iCloud")])
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let snapshot = await store.refresh(now: date("2026-09-18T10:00:00Z"), leadTime: 60)
    #expect(snapshot.calendars.first?.accountName == "iCloud")
}
