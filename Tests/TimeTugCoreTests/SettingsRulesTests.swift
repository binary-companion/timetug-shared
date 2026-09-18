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

@Test func decodingLegacyConflictLetsTakeoverWin() throws {
    let json = Data(#"{"takeoverCalendarKeys":["a"],"hiddenCalendarKeys":["a","b"]}"#.utf8)
    let s = try JSONDecoder().decode(TakeoverSettings.self, from: json)
    #expect(s.isShownInList("a"))
    #expect(s.takeoverCalendarKeys.contains("a"))
    #expect(!s.isShownInList("b"))
}
