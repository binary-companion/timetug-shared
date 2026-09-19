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

@Test func contentKeysIncludeMergedMembers() {
    var event = makeEvent("1", title: "Doctor")
    #expect(event.allContentKeys == [event.contentKey])
    event.mergedMembers = [MergedMember(title: "Intermountain Health", calendarKey: "fake/other", contentKey: "k2", details: "bare")]
    #expect(event.allContentKeys == [event.contentKey, "k2"])
}

@Test func isSameMeetingMatchesAnyMemberKey() {
    let armed = makeEvent("1", title: "Scott: Doctor")
    var merged = makeEvent("2", title: "Intermountain Health")
    #expect(!merged.isSameMeeting(as: armed))
    merged.mergedMembers = [MergedMember(title: armed.title, calendarKey: armed.calendarKey, contentKey: armed.contentKey, details: "bare")]
    #expect(merged.isSameMeeting(as: armed))
    #expect(armed.isSameMeeting(as: merged))
}

@Test func attendeeEmailNormalizationAndMailtoParsing() {
    #expect(Attendee(email: "  Kristin@Example.COM ").email == "kristin@example.com")
    #expect(Attendee(email: "   ").email == nil)
    #expect(Attendee.email(fromMailto: "mailto:Bob@Example.com?subject=x") == "bob@example.com")
    #expect(Attendee.email(fromMailto: "https://example.com/principal/1") == nil)
    #expect(Attendee.email(fromMailto: nil) == nil)
}
