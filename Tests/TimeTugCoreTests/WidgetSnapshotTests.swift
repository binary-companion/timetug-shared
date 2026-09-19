import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T12:00:00Z")

private func make(_ events: [CalendarEvent], settings: TakeoverSettings = TakeoverSettings(),
                  calendars: [CalendarInfo] = []) -> WidgetSnapshot {
    WidgetSnapshot.make(events: events, calendars: calendars, settings: settings, now: now, calendar: utcCalendar)
}

@Test func snapshotSkipsAllDayEventsWhenSettingIsOn() {
    let allDay = makeEvent("a", start: "2026-09-18T00:00:00Z", minutes: 24 * 60, isAllDay: true)
    #expect(make([allDay]).events.isEmpty)
    var settings = TakeoverSettings()
    settings.skipAllDayEvents = false
    #expect(make([allDay], settings: settings).events.map(\.id) == [allDay.id])
}

@Test func snapshotHidesEventsOnHiddenCalendars() {
    var settings = TakeoverSettings()
    settings.setShownInList(false, forCalendar: "fake/cal")
    #expect(make([makeEvent()], settings: settings).events.isEmpty)
}

@Test func snapshotCoversTodayThroughTheHorizonOnly() {
    let earlierToday = makeEvent("early", start: "2026-09-18T08:00:00Z")
    let inHorizon = makeEvent("in", start: "2026-09-20T10:00:00Z")
    let beyond = makeEvent("out", start: "2026-09-21T10:00:00Z")
    let yesterday = makeEvent("old", start: "2026-09-17T10:00:00Z")
    let ids = make([beyond, inHorizon, yesterday, earlierToday]).events.map(\.id)
    #expect(ids == [earlierToday.id, inHorizon.id])
}

@Test func snapshotSortsByStartThenTitle() {
    let b = makeEvent("b", title: "B", start: "2026-09-18T15:00:00Z")
    let a = makeEvent("a", title: "A", start: "2026-09-18T15:00:00Z")
    let early = makeEvent("e", title: "Z", start: "2026-09-18T13:00:00Z")
    #expect(make([b, a, early]).events.map(\.title) == ["Z", "A", "B"])
}

@Test func snapshotCarriesColourJoinLinkAndShownTimes() {
    let link = URL(string: "https://meet.google.com/aaa-bbbb-ccc")!
    var event = makeEvent("m", start: "2026-09-18T14:00:00Z", conferenceURL: link)
    event.displayStart = date("2026-09-18T13:30:00Z")
    let calendars = [CalendarInfo(sourceID: "fake", calendarID: "cal", title: "Work", colorHex: "#ff0000")]
    let widgetEvent = make([event], calendars: calendars).events[0]
    #expect(widgetEvent.colorHex == "#FF0000")
    #expect(widgetEvent.joinURL == link)
    #expect(widgetEvent.start == date("2026-09-18T13:30:00Z"))
    #expect(widgetEvent.end == event.end)
}

@Test func snapshotRoundTripsThroughJSON() throws {
    let snapshot = make([makeEvent()])
    let encoder = JSONEncoder(); encoder.dateEncodingStrategy = .iso8601
    let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .iso8601
    #expect(try decoder.decode(WidgetSnapshot.self, from: encoder.encode(snapshot)) == snapshot)
}

@Test func snapshotIsStaleAfterMaxAge() {
    let snapshot = WidgetSnapshot(generatedAt: now, events: [])
    #expect(!snapshot.isStale(now: now.addingTimeInterval(WidgetSnapshot.maxAge)))
    #expect(snapshot.isStale(now: now.addingTimeInterval(WidgetSnapshot.maxAge + 1)))
}

@Test func snapshotBreaksStartAndTitleTiesById() {
    let one = makeEvent("1", title: "Same", start: "2026-09-18T15:00:00Z")
    let two = makeEvent("2", title: "Same", start: "2026-09-18T15:00:00Z")
    let expected = [one.id, two.id].sorted()
    #expect(make([one, two]).events.map(\.id) == expected)
    #expect(make([two, one]).events.map(\.id) == expected)
}

@Test func snapshotExcludesEventEndingExactlyAtStartOfToday() {
    let ends = makeEvent("x", start: "2026-09-17T23:30:00Z")
    #expect(make([ends]).events.isEmpty)
}

@Test func snapshotExcludesEventStartingExactlyAtHorizonEnd() {
    let starts = makeEvent("x", start: "2026-09-21T00:00:00Z")
    #expect(make([starts]).events.isEmpty)
}

@Test func snapshotIncludesEventStartedYesterdayAndStillRunning() {
    let running = makeEvent("x", start: "2026-09-17T23:00:00Z", minutes: 120)
    #expect(make([running]).events.map(\.id) == [running.id])
}

@Test func snapshotHidesMergedEventOnlyWhenAllItsCalendarsAreHidden() {
    var merged = makeEvent("m")
    merged.additionalCalendarKeys = ["fake/other"]
    var settings = TakeoverSettings()
    settings.setShownInList(false, forCalendar: "fake/cal")
    #expect(make([merged], settings: settings).events.map(\.id) == [merged.id])
    settings.setShownInList(false, forCalendar: "fake/other")
    #expect(make([merged], settings: settings).events.isEmpty)
}
