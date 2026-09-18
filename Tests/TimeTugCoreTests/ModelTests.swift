import Foundation
import Testing
@testable import TimeTugCore

@Test func eventIdIncludesStartSoRecurringInstancesDiffer() {
    let a = makeEvent("series", start: "2026-09-18T10:00:00Z")
    let b = makeEvent("series", start: "2026-09-19T10:00:00Z")
    #expect(a.id != b.id)
}

@Test func calendarKeyCombinesSourceAndCalendar() {
    #expect(makeEvent(calendarID: "work").calendarKey == "fake/work")
    #expect(CalendarInfo.key(sourceID: "fake", calendarID: "work") == "fake/work")
}

@Test func settingsRoundTripThroughJSON() throws {
    var settings = TakeoverSettings()
    settings.leadTime = 300
    settings.takeoverCalendarKeys = ["a/b"]
    let data = try JSONEncoder().encode(settings)
    #expect(try JSONDecoder().decode(TakeoverSettings.self, from: data) == settings)
}

@Test func settingsDefaults() {
    let settings = TakeoverSettings()
    #expect(settings.leadTime == 60)
    #expect(settings.skipSoloEvents && settings.skipDeclinedEvents)
    #expect(!settings.requireConferenceLink)
    #expect(settings.takeoverCalendarKeys.isEmpty)
    #expect(settings.skipAllDayEvents)
}

@Test func settingsDecodeOldFormatJSONWithoutNewKeys() throws {
    let json = Data(#"{"leadTime":300,"takeoverCalendarKeys":["a/b"]}"#.utf8)
    let settings = try JSONDecoder().decode(TakeoverSettings.self, from: json)
    #expect(settings.leadTime == 300)
    #expect(settings.takeoverCalendarKeys == ["a/b"])
    #expect(settings.hiddenCalendarKeys.isEmpty)
    #expect(!settings.requireConferenceLink)
    #expect(settings.skipSoloEvents)
    #expect(settings.skipDeclinedEvents)
    #expect(settings.skipAllDayEvents)
}
