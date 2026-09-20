import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T10:00:00Z")

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
    nonisolated func changes() -> AsyncStream<Void> { AsyncStream { $0.finish() } }
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
