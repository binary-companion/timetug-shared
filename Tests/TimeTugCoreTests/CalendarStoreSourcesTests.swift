import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T10:00:00Z")

private actor SnapshotRecorder {
    private(set) var snapshots: [CalendarSnapshot] = []
    func record(_ snapshot: CalendarSnapshot) { snapshots.append(snapshot) }
    func contains(_ id: String) -> Bool { snapshots.contains { $0.events.contains { $0.sourceEventID == id } } }
}

@Test func targetedRefreshReadsOnlyTheNamedSource() async {
    let a = FakeSource(id: "a"), b = FakeSource(id: "b"), c = FakeSource(id: "c")
    let store = CalendarStore(sources: [a, b, c], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 60)
    await a.set(events: .success([makeEvent("changed")]))
    let snapshot = await store.refresh(now: now, leadTime: 60, sourceIDs: ["a"])
    #expect(snapshot.events.map(\.sourceEventID) == ["changed"])
    #expect(await a.requestedIntervals.count == 2)
    #expect(await b.requestedIntervals.count == 1)
    #expect(await c.requestedIntervals.count == 1)
}

@Test func healthySourcePublishesWhileAnotherSourceIsStalled() async {
    let a = GateSource(id: "a", events: [makeEvent("slow")])
    let b = FakeSource(id: "b")
    await b.set(events: .success([makeEvent("fast", title: "Fast", start: "2026-09-18T12:00:00Z")]))
    let store = CalendarStore(sources: [a, b], calendar: utcCalendar)
    let recorder = SnapshotRecorder()
    let refreshing = Task {
        await store.refresh(now: now, leadTime: 60, onPublication: { await recorder.record($0) })
    }
    var observed = false
    for _ in 0..<500 {
        if await recorder.contains("fast") { observed = true; break }
        await Task.yield()
    }
    #expect(observed)
    await a.release()
    let final = await refreshing.value
    #expect(Set(final.events.map(\.sourceEventID)) == ["slow", "fast"])
}

@Test func olderTargetedRequestCannotOverwriteNewerSourceData() async {
    let source = OrderedSource()
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let old = Task { await store.refresh(now: now, leadTime: 60, sourceIDs: [source.id]) }
    await source.waitForRequest(1)
    let new = Task { await store.refresh(now: now, leadTime: 60, sourceIDs: [source.id]) }
    await source.waitForRequest(2)
    await source.finish(1, events: [makeEvent("new", title: "New")])
    #expect(await new.value.events.map(\.sourceEventID) == ["new"])
    await source.finish(0, events: [makeEvent("old", title: "Old")])
    _ = await old.value
    #expect(await store.setInferenceEnabled(false, now: now).events.map(\.sourceEventID) == ["new"])
}

/// A source whose `events(in:)` waits until `release()` so a test can interleave `setSources`.
actor GateSource: CalendarSource {
    nonisolated let id: String
    nonisolated let displayName = "Gate"
    private var waiter: CheckedContinuation<Void, Never>?
    private var isOpen = false
    private(set) var started = false
    private let stored: [TimeTugCalendarEvent]

    init(id: String, events: [TimeTugCalendarEvent]) { self.id = id; stored = events }

    func calendars() async throws -> [CalendarInfo] { [] }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] {
        started = true
        if !isOpen { await withCheckedContinuation { waiter = $0 } }
        return stored
    }
    func release() { isOpen = true; waiter?.resume(); waiter = nil }
    nonisolated func changes() -> AsyncStream<SourceChange> { AsyncStream { $0.finish() } }
}

private actor OrderedSource: CalendarSource {
    nonisolated let id = "ordered"
    nonisolated let displayName = "Ordered"
    private var waiters: [CheckedContinuation<[TimeTugCalendarEvent], Never>] = []
    private var milestones: [Int: CheckedContinuation<Void, Never>] = [:]
    func calendars() async throws -> [CalendarInfo] { [] }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] {
        await withCheckedContinuation {
            waiters.append($0)
            milestones.removeValue(forKey: waiters.count)?.resume()
        }
    }
    func waitForRequest(_ count: Int) async {
        if waiters.count >= count { return }
        await withCheckedContinuation { milestones[count] = $0 }
    }
    func finish(_ request: Int, events: [TimeTugCalendarEvent]) { waiters[request].resume(returning: events) }
    nonisolated func changes() -> AsyncStream<SourceChange> { AsyncStream { $0.finish() } }
}

@Test func olderRefreshCannotOverwriteNewerRefreshOrWindow() async throws {
    let source = OrderedSource()
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let old = Task { await store.refresh(now: date("2026-09-18T23:59:00Z"), leadTime: 60) }
    await source.waitForRequest(1)
    let new = Task { await store.refresh(now: date("2026-09-19T00:01:00Z"), leadTime: 60) }
    await source.waitForRequest(2)
    await source.finish(1, events: [makeEvent("new", title: "NEW", start: "2026-09-19T12:00:00Z")])
    #expect(await new.value.events.map(\.title) == ["NEW"])
    await source.finish(0, events: [makeEvent("old", title: "OLD", start: "2026-09-18T12:00:00Z")])
    _ = await old.value
    let final = await store.setInferenceEnabled(false, now: date("2026-09-19T00:01:00Z"))
    #expect(final.events.map(\.title) == ["NEW"])
    #expect(await store.fetchWindow(now: date("2026-09-19T00:01:00Z"), leadTime: 60).start
            == date("2026-09-19T00:00:00Z"))
}

@Test func timezoneChangeDuringRefreshKeepsNewDSTDayAndFarZoneAllDay() async {
    let source = OrderedSource()
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let instant = date("2026-03-08T10:00:00Z")
    let old = Task { await store.refresh(now: instant, leadTime: 60) }
    await source.waitForRequest(1)
    await store.setCalendar(calendar(in: "America/Denver"))
    let current = Task { await store.refresh(now: instant, leadTime: 60) }
    await source.waitForRequest(2)
    let farZoneDay = makeAllDay(zone: "Pacific/Midway", first: day(2026, 3, 8), endExclusive: day(2026, 3, 9))
    await source.finish(1, events: [makeEvent("new", start: "2026-03-08T12:00:00Z"), farZoneDay])
    let accepted = await current.value
    #expect(Set(accepted.events.map(\.sourceEventID)) == ["new", "d"])
    await source.finish(0, events: [makeEvent("old", start: "2026-03-08T02:00:00Z")])
    _ = await old.value
    let final = await store.setInferenceEnabled(false, now: instant)
    #expect(Set(final.events.map(\.sourceEventID)) == ["new", "d"])
    #expect(await store.fetchWindow(now: instant, leadTime: 60).start == date("2026-03-08T07:00:00Z"))
}

@Test func setSourcesDropsRemovedSourceState() async {
    let a = FakeSource(id: "a"), b = FakeSource(id: "b")
    await a.set(events: .success([makeEvent("1")]))
    await b.set(events: .success([makeEvent("2", title: "Other", start: "2026-09-18T12:00:00Z")]))
    let store = CalendarStore(sources: [a, b], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 60)
    await store.setSources([a])
    // setInferenceEnabled returns a snapshot built from cached state without refreshing.
    let snapshot = await store.setInferenceEnabled(false, now: now)
    #expect(snapshot.events.map(\.sourceEventID) == ["1"])
    #expect(Set(snapshot.statuses.keys) == ["a"])
    #expect(Set(snapshot.sourceNames.keys) == ["a"])
}

@Test func setSourcesAddsASourceForTheNextRefresh() async {
    let a = FakeSource(id: "a"), b = FakeSource(id: "b")
    await b.set(events: .success([makeEvent("2", title: "Other", start: "2026-09-18T12:00:00Z")]))
    let store = CalendarStore(sources: [a], calendar: utcCalendar)
    await store.setSources([a, b])
    let snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.events.map(\.sourceEventID) == ["2"])
    #expect(Set(snapshot.statuses.keys) == ["a", "b"])
}

@Test func refreshInFlightDuringSetSourcesDoesNotResurrectARemovedSource() async throws {
    let gate = GateSource(id: "eventkit", events: [makeEvent("1")])
    let store = CalendarStore(sources: [gate], calendar: utcCalendar)
    let refreshing = Task { await store.refresh(now: now, leadTime: 60) }
    for _ in 0..<400 {
        if await gate.started { break }
        try await Task.sleep(for: .milliseconds(5))
    }
    #expect(await gate.started)
    await store.setSources([])
    await gate.release()
    let snapshot = await refreshing.value
    #expect(snapshot.events.isEmpty)
    #expect(snapshot.statuses.isEmpty)
    // Toggled back on: nothing stale shows until its next refresh.
    await store.setSources([gate])
    #expect(await store.setInferenceEnabled(false, now: now).events.isEmpty)
}

@Test func aRemovedThenReAddedSourceShowsNothingUntilItsNextRefresh() async {
    let a = FakeSource(id: "a"), b = FakeSource(id: "b")
    await b.set(events: .success([makeEvent("2", title: "Other", start: "2026-09-18T12:00:00Z")]))
    let store = CalendarStore(sources: [a, b], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 60)
    await store.setSources([a])
    await store.setSources([a, b])
    let snapshot = await store.setInferenceEnabled(false, now: now)
    #expect(snapshot.events.isEmpty)
    #expect(snapshot.statuses["b"] == nil)
}
