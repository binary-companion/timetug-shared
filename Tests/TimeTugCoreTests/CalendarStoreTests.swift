import Foundation
import Testing
@testable import TimeTugCore

actor FakeSource: CalendarSource {
    nonisolated let id: String
    nonisolated let displayName: String
    var calendarsResult: Result<[CalendarInfo], Error> = .success([])
    var eventsResult: Result<[CalendarEvent], Error> = .success([])
    private(set) var requestedIntervals: [DateInterval] = []

    init(id: String = "fake") {
        self.id = id
        self.displayName = "Fake \(id)"
    }

    func set(events: Result<[CalendarEvent], Error>) { eventsResult = events }
    func set(calendars: [CalendarInfo]) { calendarsResult = .success(calendars) }

    func calendars() async throws -> [CalendarInfo] { try calendarsResult.get() }
    func events(in interval: DateInterval) async throws -> [CalendarEvent] {
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
    #expect(await source.requestedIntervals.first?.end == date("2026-09-19T00:06:00Z"))
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
