import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct LaunchAcknowledgementTests {
    private let launch = Date(timeIntervalSince1970: 1_800_000_000)

    private func event(_ id: String, source: String, start: Date, end: Date) -> TimeTugCalendarEvent {
        TimeTugCalendarEvent(event: CalendarCore.CalendarEvent(
            eventID: id, calendarID: "main", title: id, start: start, end: end), sourceID: source)
    }

    private func snapshot(_ events: [TimeTugCalendarEvent], _ statuses: [String: SourceStatus]) -> CalendarSnapshot {
        CalendarSnapshot(events: events, calendars: [], statuses: statuses, sourceNames: [:], fetchedAt: launch)
    }

    @Test func recoveryAcknowledgesOnlyPrelaunchMeetingsOnHealthySources() {
        var state = LaunchAcknowledgement(launchedAt: launch)
        state.registerSources(["a", "b"], now: launch.addingTimeInterval(5))
        let recovered = launch.addingTimeInterval(15 * 60)
        let oldA = event("old-a", source: "a", start: launch.addingTimeInterval(-600), end: recovered.addingTimeInterval(600))
        let oldB = event("old-b", source: "b", start: launch.addingTimeInterval(-600), end: recovered.addingTimeInterval(600))
        let newA = event("new-a", source: "a", start: launch.addingTimeInterval(300), end: recovered.addingTimeInterval(600))
        let events = [oldA, oldB, newA]
        #expect(state.eventsToAcknowledge(in: snapshot(events, ["a": .ok, "b": .authExpired]),
                                          now: recovered, grace: 120).map(\.id) == [oldA.id])
        #expect(state.eventsToAcknowledge(in: snapshot(events, ["a": .ok, "b": .ok]),
                                          now: recovered, grace: 120).map(\.id) == [oldB.id])
    }

    @Test func addedSourceUsesAdditionTimeAndRemovedSourceIsDropped() {
        var state = LaunchAcknowledgement(launchedAt: launch)
        state.registerSources(["a"], now: launch)
        _ = state.eventsToAcknowledge(in: snapshot([], ["a": .ok]), now: launch, grace: 120)
        let addedAt = launch.addingTimeInterval(3600)
        state.registerSources(["a", "b"], now: addedAt)
        let old = event("old", source: "b", start: addedAt.addingTimeInterval(-300), end: addedAt.addingTimeInterval(600))
        #expect(state.eventsToAcknowledge(in: snapshot([old], ["b": .ok]),
                                          now: addedAt, grace: 120).map(\.id) == [old.id])
        state.registerSources(["a"], now: addedAt)
        #expect(state.eventsToAcknowledge(in: snapshot([old], ["b": .ok]), now: addedAt, grace: 120).isEmpty)
    }
}
