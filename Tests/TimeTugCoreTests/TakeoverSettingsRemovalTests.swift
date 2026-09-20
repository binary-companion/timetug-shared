import Testing
@testable import TimeTugCore

@Test func removeCalendarsForSourceDropsOnlyThatSourcesKeys() {
    var s = TakeoverSettings()
    s.takeoverCalendarKeys = ["google-1/a", "google-10/a", "eventkit/x"]
    s.hiddenCalendarKeys = ["google-1/b", "eventkit/y"]
    s.removeCalendars(forSourceID: "google-1")
    #expect(s.takeoverCalendarKeys == ["google-10/a", "eventkit/x"])
    #expect(s.hiddenCalendarKeys == ["eventkit/y"])
}

@Test func removeCalendarsForUnknownSourceChangesNothing() {
    var s = TakeoverSettings()
    s.takeoverCalendarKeys = ["eventkit/x"]
    s.removeCalendars(forSourceID: "google-1")
    #expect(s.takeoverCalendarKeys == ["eventkit/x"])
}

@Test func removeCalendarsWhereSourceMatchesPredicateUsesTextBeforeFirstSlash() {
    var s = TakeoverSettings()
    s.takeoverCalendarKeys = ["google-a/cal/with/slashes", "google-b/c", "eventkit/x"]
    s.removeCalendars(whereSourceID: { $0.hasPrefix("google-") && $0 != "google-b" })
    #expect(s.takeoverCalendarKeys == ["google-b/c", "eventkit/x"])
}
