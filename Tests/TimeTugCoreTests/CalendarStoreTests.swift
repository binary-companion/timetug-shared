import Foundation
import Testing
@testable import TimeTugCore

actor FakeSource: CalendarSource {
    nonisolated let id: String
    nonisolated let displayName: String
    var calendarsResult: Result<[CalendarInfo], Error> = .success([])
    var eventsResult: Result<[TimeTugCalendarEvent], Error> = .success([])
    private(set) var requestedIntervals: [DateInterval] = []

    init(id: String = "fake") {
        self.id = id
        self.displayName = "Fake \(id)"
    }

    func set(events: Result<[TimeTugCalendarEvent], Error>) { eventsResult = events }
    func set(calendars: [CalendarInfo]) { calendarsResult = .success(calendars) }

    func calendars() async throws -> [CalendarInfo] { try calendarsResult.get() }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] {
        requestedIntervals.append(interval)
        return try eventsResult.get()
    }
    nonisolated func changes() -> AsyncStream<Void> { AsyncStream { $0.finish() } }
}

private let now = date("2026-09-18T10:00:00Z")

@Test func fetchWindowSpansTodayPlusLeadTimeAndBuffer() async {
    let store = CalendarStore(sources: [], calendar: utcCalendar)
    let window = await store.fetchWindow(now: now, leadTime: 600)
    #expect(window.start == date("2026-09-18T00:00:00Z"))
    #expect(window.end == date("2026-09-19T00:15:00Z"))   // midnight + 600s + 300s buffer
}

@Test func refreshAsksSourcesForFetchWindow() async {
    let source = FakeSource()
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 60)
    #expect(await source.requestedIntervals.first?.end
        == date("2026-09-19T00:06:00Z").addingTimeInterval(CalendarStore.sourceQueryMargin))
}

@Test func mergesSortsAndDedupesAcrossCalendars() async {
    let a = FakeSource(id: "a"), b = FakeSource(id: "b")
    let dup1 = makeEvent("1", title: "Design Review", start: "2026-09-18T11:00:00Z")
    var dup2 = makeEvent("9", title: "design review", start: "2026-09-18T11:00:00Z")
    dup2.sourceID = "b"
    let early = makeEvent("2", title: "Standup", start: "2026-09-18T09:00:00Z")
    await a.set(events: .success([dup1, early]))
    await b.set(events: .success([dup2]))
    let store = CalendarStore(sources: [a, b], calendar: utcCalendar)
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.events.map(\.title) == ["Standup", "Design Review"])
}

@Test func fillsConferenceURLFromDetectorWhenSourceHasNone() async {
    let source = FakeSource()
    await source.set(events: .success([makeEvent(location: "https://acme.zoom.us/j/5")]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.events.first?.conferenceURL?.host == "acme.zoom.us")
}

@Test func keepsSourceSuppliedConferenceURL() async {
    let structured = URL(string: "https://meet.google.com/aaa-bbbb-ccc")
    let source = FakeSource()
    await source.set(events: .success([makeEvent(location: "https://acme.zoom.us/j/5", conferenceURL: structured)]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    #expect(await store.refresh(now: now, leadTime: 60).events.first?.conferenceURL == structured)
}

@Test func failingSourceKeepsLastGoodEventsAndReportsStatus() async {
    struct Boom: Error {}
    let source = FakeSource()
    await source.set(events: .success([makeEvent()]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 60)

    await source.set(events: .failure(Boom()))
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.events.count == 1)
    if case .failing = snapshot.statuses["fake"] {} else { Issue.record("expected .failing") }
}

@Test func permissionAndAuthErrorsMapToStatuses() async {
    let source = FakeSource()
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    await source.set(events: .failure(SourceError.needsPermission))
    #expect(await store.refresh(now: now, leadTime: 60).statuses["fake"] == .needsPermission)
    await source.set(events: .failure(SourceError.authExpired))
    #expect(await store.refresh(now: now, leadTime: 60).statuses["fake"] == .authExpired)
}

@Test func oneFailingSourceDoesNotAffectAnother() async {
    let bad = FakeSource(id: "bad"), good = FakeSource(id: "good")
    await bad.set(events: .failure(SourceError.needsPermission))
    var event = makeEvent()
    event.sourceID = "good"
    await good.set(events: .success([event]))
    let store = CalendarStore(sources: [bad, good], calendar: utcCalendar)
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.events.count == 1)
    #expect(snapshot.statuses["good"] == .ok)
}

@Test func dropsEventsOutsideFetchWindow() async {
    let yesterday = makeEvent("y", start: "2026-09-17T10:00:00Z")
    let source = FakeSource()
    await source.set(events: .success([yesterday, makeEvent("t")]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    #expect(await store.refresh(now: now, leadTime: 60).events.map(\.sourceEventID) == ["t"])
}

@Test func snapshotIncludesCalendarsAndSourceNames() async {
    let source = FakeSource()
    await source.set(calendars: [CalendarInfo(sourceID: "fake", calendarID: "cal", title: "Work")])
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.calendars.map(\.title) == ["Work"])
    #expect(snapshot.sourceNames["fake"] == "Fake fake")
}

@Test func duplicateKeepsFirstCopyButRecordsOtherCalendarKeys() async {
    let a = FakeSource(id: "fake"), b = FakeSource(id: "fake2")
    let first = makeEvent("1", title: "Sync", calendarID: "family")
    var second = makeEvent("2", title: "Sync", calendarID: "work")
    second.sourceID = "fake/work-src"
    await a.set(events: .success([first]))
    await b.set(events: .success([second]))
    let store = CalendarStore(sources: [a, b], calendar: utcCalendar)
    let events = await store.refresh(now: now, leadTime: 60).events
    #expect(events.count == 1)
    #expect(events.first?.calendarKey == "fake/family")
    #expect(events.first?.additionalCalendarKeys.contains("fake/work-src/work") == true)
}

@Test func duplicateFillsMissingFieldsFromLaterCopy() async {
    let a = FakeSource(id: "a"), b = FakeSource(id: "b")
    let first = makeEvent("1", title: "Sync")
    var second = makeEvent("2", title: "Sync", notes: "Join https://acme.zoom.us/j/123")
    second.sourceID = "b"
    await a.set(events: .success([first]))
    await b.set(events: .success([second]))
    let store = CalendarStore(sources: [a, b], calendar: utcCalendar)
    let events = await store.refresh(now: now, leadTime: 60).events
    #expect(events.count == 1)
    #expect(events.first?.conferenceURL?.host?.hasSuffix("zoom.us") == true)
}

@Test func sameCalendarImportedFromEverySourceCollapsesToOneCard() async {
    // The user's Exchange, Gmail and iCloud calendars share a name; a hidden calendar still takes part in the merge.
    let names = ["exchange", "gmail", "cloud", "cloud2", "cloud3"]
    var sources: [FakeSource] = []
    for name in names {
        let source = FakeSource(id: name)
        var event = makeEvent("\(name)-1", title: "O'Bryan Family Dinner", start: "2026-09-18T11:00:00Z", calendarID: "shared-\(name)")
        event.sourceID = name
        await source.set(events: .success([event]))
        await source.set(calendars: [CalendarInfo(sourceID: name, calendarID: "shared-\(name)", title: "O'Bryan Shared")])
        sources.append(source)
    }
    let store = CalendarStore(sources: sources, calendar: utcCalendar)
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.events.count == 1)
    let card = snapshot.events[0]
    #expect(card.mergedMembers.count == 5)
    #expect(card.additionalCalendarKeys.count == 4)
    #expect(card.allCalendarKeys == Set(names.map { "\($0)/shared-\($0)" }))
    // Hiding is a display concern: one visible copy keeps the merged card on the agenda.
    var settings = TakeoverSettings()
    settings.hiddenCalendarKeys = Set(names.dropFirst().map { "\($0)/shared-\($0)" })
    let agenda = DayAgenda.make(events: snapshot.events, settings: settings, now: now, calendar: utcCalendar)
    #expect(agenda.items.count == 1)
}

@Test func sourcesAreQueriedWithAZoneMargin() async {
    let source = FakeSource()
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 600)
    let window = await store.fetchWindow(now: now, leadTime: 600)
    let requested = await source.requestedIntervals.first
    #expect(requested?.start == window.start.addingTimeInterval(-CalendarStore.sourceQueryMargin))
    #expect(requested?.end == window.end.addingTimeInterval(CalendarStore.sourceQueryMargin))
}

@Test func farZoneAllDayEventOnTodaysDateIsKept() async {
    // Viewer in UTC+14, calendar in UTC-11: the event's instants start after the viewer's day window ends.
    let kiritimati = calendar(in: "Pacific/Kiritimati")
    let midway = makeAllDay(zone: "Pacific/Midway", first: day(2026, 9, 18), endExclusive: day(2026, 9, 19))
    let source = FakeSource()
    await source.set(events: .success([midway]))
    let store = CalendarStore(sources: [source], calendar: kiritimati)
    let snapshot = await store.refresh(now: date("2026-09-18T02:00:00Z"), leadTime: 60)   // Sep 18 16:00 there
    #expect(snapshot.events.map(\.sourceEventID) == ["d"])
}

@Test func manualMergeWorksWhenOneCopyOfTheFirstEventSharesACalendarWithTheSecond() async {
    // The booking copy of a meeting and its placeholder sit on one shared calendar, and the booking is also
    // copied onto other calendars. The user says they are one meeting: the same-calendar copy must not veto it.
    let zoom = URL(string: "https://acme.zoom.us/j/1")!
    let elsewhere = makeEvent("1", title: "Consultation with Michael", calendarID: "personal", notes: "n", conferenceURL: zoom)
    let onShared = makeEvent("2", title: "Consultation with Michael", calendarID: "shared", notes: "n", conferenceURL: zoom)
    let placeholder = makeEvent("3", title: "Scott: Careerminds consultation", calendarID: "shared", others: 0)
    let source = FakeSource()
    await source.set(events: .success([elsewhere, onShared, placeholder]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let before = await store.refresh(now: now, leadTime: 60)
    #expect(before.events.count == 2)
    let after = await store.merge(before.events[0], before.events[1], now: now)
    #expect(after.events.count == 1)
    #expect(after.events[0].mergeProvenance == .userConfirmed)
    #expect(after.events[0].mergedMembers.count == 3)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 1)   // and it sticks on the next refresh
}
