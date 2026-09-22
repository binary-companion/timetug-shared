import Foundation
import Testing
@testable import TimeTugCore

@Test func enablingTakeoverShowsHiddenCalendar() {
    var s = TakeoverSettings()
    s.hiddenCalendarKeys = ["a"]
    s.setTakeover(true, forCalendar: "a")
    #expect(s.isShownInList("a"))
    #expect(s.takeoverCalendarKeys.contains("a"))
}

@Test func hidingCalendarTurnsTakeoverOff() {
    var s = TakeoverSettings()
    s.setTakeover(true, forCalendar: "a")
    s.setShownInList(false, forCalendar: "a")
    #expect(!s.isShownInList("a"))
    #expect(!s.takeoverCalendarKeys.contains("a"))
}

@Test func showingCalendarDoesNotEnableTakeover() {
    var s = TakeoverSettings()
    s.setShownInList(false, forCalendar: "a")
    s.setShownInList(true, forCalendar: "a")
    #expect(s.isShownInList("a"))
    #expect(!s.takeoverCalendarKeys.contains("a"))
}

@Test func disablingTakeoverLeavesCalendarShown() {
    var s = TakeoverSettings()
    s.setTakeover(true, forCalendar: "a")
    s.setTakeover(false, forCalendar: "a")
    #expect(s.isShownInList("a"))
    #expect(!s.takeoverCalendarKeys.contains("a"))
}

@Test func decodingConflictLetsHidingWin() throws {
    // A calendar that is not shown never tugs.
    let json = Data(#"{"takeoverCalendarKeys":["a","c"],"hiddenCalendarKeys":["a","b"]}"#.utf8)
    let s = try JSONDecoder().decode(TakeoverSettings.self, from: json)
    #expect(!s.isShownInList("a"))
    #expect(!s.takeoverCalendarKeys.contains("a"))
    #expect(s.takeoverCalendarKeys.contains("c"))
    #expect(!s.isShownInList("b"))
}

@Test func enabledDefaultsToTrueWhenMissingFromSavedJSON() throws {
    let decoded = try JSONDecoder().decode(TakeoverSettings.self, from: Data("{}".utf8))
    #expect(decoded.enabled)
}

@Test func enabledRoundTripsThroughJSON() throws {
    var settings = TakeoverSettings()
    settings.enabled = false
    let decoded = try JSONDecoder().decode(TakeoverSettings.self, from: JSONEncoder().encode(settings))
    #expect(!decoded.enabled)
}

@Test func legacyDisabledFlagMigratesToEnabled() throws {
    let off = try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"disabled":true}"#.utf8))
    #expect(!off.enabled)
    let on = try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"disabled":false}"#.utf8))
    #expect(on.enabled)
    // The new key wins when both are present.
    let both = try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"disabled":true,"enabled":true}"#.utf8))
    #expect(both.enabled)
}

@Test func legacySkipSoloFlagMigratesToRequireOtherAttendees() throws {
    let on = try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"skipSoloEvents":true}"#.utf8))
    #expect(on.requireOtherAttendees)
    let off = try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"skipSoloEvents":false}"#.utf8))
    #expect(!off.requireOtherAttendees)
    let both = try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"skipSoloEvents":true,"requireOtherAttendees":false}"#.utf8))
    #expect(!both.requireOtherAttendees)
}

@Test func savedSettingsNoLongerWriteRetiredKeys() throws {
    let json = try #require(String(data: JSONEncoder().encode(TakeoverSettings()), encoding: .utf8))
    #expect(!json.contains("disabled") && !json.contains("skipSoloEvents") && !json.contains("skipDeclinedEvents"))
}

@Test func settingTugForSeveralCalendarsAlsoShowsThemAndClearsIt() {
    var s = TakeoverSettings()
    s.hiddenCalendarKeys = ["a", "b"]
    s.setTakeover(true, forCalendars: ["a", "b", "c"])
    #expect(s.takeoverCalendarKeys == ["a", "b", "c"])
    #expect(s.hiddenCalendarKeys.isEmpty)
    s.setTakeover(false, forCalendars: ["a", "b"])
    #expect(s.takeoverCalendarKeys == ["c"])
    #expect(s.isShownInList("a"))
}
