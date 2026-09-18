import Testing
import Foundation
@testable import TimeTugCore

@Test(arguments: [
    ("#2f7bf5", "#2F7BF5"), ("2F7BF5", "#2F7BF5"), ("#abc", "#AABBCC"),
    (" #FF0000 ", "#FF0000"), ("abc", "#AABBCC"),
])
func normalizedHexAcceptsValidForms(raw: String, expected: String) {
    #expect(CalendarInfo.normalizedHex(raw) == expected)
}

@Test(arguments: ["zzz", "#12", "#1234567", "", "#GG0000", "  "])
func normalizedHexRejectsInvalid(raw: String) {
    #expect(CalendarInfo.normalizedHex(raw) == nil)
}

@Test func normalizedHexNilIsNil() {
    #expect(CalendarInfo.normalizedHex(nil) == nil)
}

@Test func colorHexDefaultsToNilAndIsNormalizedByInit() {
    #expect(CalendarInfo(sourceID: "s", calendarID: "c", title: "T").colorHex == nil)
    #expect(CalendarInfo(sourceID: "s", calendarID: "c", title: "T", colorHex: "#2f7bf5").colorHex == "#2F7BF5")
    #expect(CalendarInfo(sourceID: "s", calendarID: "c", title: "T", colorHex: "bogus").colorHex == nil)
}

@Test func keyIgnoresColorButEqualityDoesNot() {
    let a = CalendarInfo(sourceID: "s", calendarID: "c", title: "T", colorHex: "#FF0000")
    let b = CalendarInfo(sourceID: "s", calendarID: "c", title: "T", colorHex: "#00FF00")
    #expect(a.key == b.key && a.id == "s/c")
    #expect(a != b)
}

@Test func refreshPreservesCalendarColor() async {
    let source = FakeSource()
    let info = CalendarInfo(sourceID: "fake", calendarID: "c1", title: "Work", accountName: "iCloud", colorHex: "#2F7BF5")
    await source.set(calendars: [info])
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let snapshot = await store.refresh(now: date("2026-09-18T10:00:00Z"), leadTime: 60)
    #expect(snapshot.calendars.first?.colorHex == "#2F7BF5")
}
