import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

private let tokyo: Calendar = {
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "Asia/Tokyo")!
    return calendar
}()

/// The start of the interval the store asks sources for, for "now" in `calendar`.
private func queryStart(_ calendar: Calendar, now: Date) -> Date {
    calendar.startOfDay(for: now).addingTimeInterval(-CalendarStore.sourceQueryMargin)
}

@Suite struct EngineSystemEventTests {
    private func started(_ events: [CalendarCore.CalendarEvent]) async throws -> EngineHarness {
        let h = try await EngineHarness(events: events)
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok }
        await h.settle()
        return h
    }

    // Scenario 5
    @Test(arguments: [SystemEvent.wake, .clockChanged, .timeZoneChanged, .dayChanged])
    func calendarEventsAdoptTheHostCalendarThenRefresh(_ event: SystemEvent) async throws {
        let h = try await started([.sample(id: "e1", startsIn: 600)])
        let base = h.source().eventsCalls
        #expect(h.source().lastInterval?.start == queryStart(EngineHarness.utc, now: h.clock.now()))

        h.hostCalendar.set(tokyo)
        await h.engine.send(.system(event))
        #expect(h.source().eventsCalls == base + 1)
        #expect(h.source().lastInterval?.start == queryStart(tokyo, now: h.clock.now()))
    }

    @Test func networkReturnRefreshesWithoutChangingTheCalendar() async throws {
        let h = try await started([.sample(id: "e1", startsIn: 600)])
        let base = h.source().eventsCalls
        h.hostCalendar.set(tokyo)
        await h.engine.send(.system(.networkReturned))
        #expect(h.source().eventsCalls == base + 1)
        #expect(h.source().lastInterval?.start == queryStart(EngineHarness.utc, now: h.clock.now()))
    }

    @Test func displayChangeRepublishesWithoutRefreshing() async throws {
        let h = try await started([.sample(id: "e1", startsIn: 600)])
        let base = h.source().eventsCalls
        let before = await h.engine.currentState()
        await h.engine.send(.system(.displayChanged))
        let after = await h.engine.currentState()
        #expect(after.revision == before.revision + 1)
        #expect(after.agenda == before.agenda)
        await h.settle()
        #expect(h.source().eventsCalls == base)
    }

    // Scenario 7
    @Test func countdownTicksRepublishEveryMinuteWithTheSameAgenda() async throws {
        let h = try await started([.sample(id: "e1", startsIn: 3600)])
        let start = await h.engine.currentState()

        // Hidden: the next wake is the "soon" window at +3000, so a minute passes without a publish.
        await h.clock.advance(by: 60)
        await h.settle()
        #expect(await h.engine.currentState().revision == start.revision)

        await h.engine.send(.setCountdownVisible(true))
        for minute in 1...2 {
            await h.clock.advance(by: 60)
            let state = await h.waitForState { $0.revision > start.revision + UInt64(minute) - 1 }
            await h.settle()
            #expect(await h.engine.currentState().revision == start.revision + UInt64(minute))
            #expect(state.agenda == start.agenda)
        }

        await h.engine.send(.setCountdownVisible(false))
        let hidden = await h.engine.currentState().revision
        await h.clock.advance(by: 60)
        await h.settle()
        #expect(await h.engine.currentState().revision == hidden)
    }

    @Test func aTickAtAnEventBoundaryRebuildsTheAgenda() async throws {
        let h = try await started([.sample(id: "e1", startsIn: 600)])
        // No takeover publishes in the way: only ticks publish from here on.
        await h.engine.send(.applySettings { $0.takeover.enabled = false })
        #expect(await h.engine.currentState().agenda.items.first?.state == .upcoming)

        // +540: the agenda lead boundary publishes; the meeting is still upcoming.
        let beforeLead = await h.engine.currentState().revision
        await h.clock.advance(by: 540)
        let lead = await h.waitForState { $0.revision > beforeLead }
        #expect(lead.agenda.items.first?.state == .upcoming)
        await h.settle()
        let beforeStart = await h.engine.currentState().revision

        // +600: the meeting starts, and the tick rebuilds the agenda.
        await h.clock.advance(by: 60)
        let started = await h.waitForState { $0.revision > beforeStart }
        #expect(started.agenda.items.first?.state == .current)
        #expect(started.agenda.next == nil)
    }

    // Scenario 8
    @Test func aWidgetSnapshotIsEmittedForEveryAppliedSnapshotEvenWhenUnchanged() async throws {
        let h = try await started([.sample(id: "e1", startsIn: 600)])
        let state = await h.engine.currentState()
        let expected = WidgetSnapshot.make(
            events: state.events, calendars: state.calendars, settings: state.settings.takeover,
            now: h.clock.now(), calendar: EngineHarness.utc)
        let before = h.widgetSnapshots.count
        #expect(before > 0)
        #expect(h.widgetSnapshots.last == expected)

        await h.engine.send(.refresh)
        // Unchanged events are still emitted (the host decides whether to reload timelines); the count is not
        // asserted exactly, only that one more arrived and that it equals the previous one.
        await h.waitUntil("the snapshot after the refresh") { h.widgetSnapshots.count > before }
        await h.settle()
        #expect(h.widgetSnapshots.last == expected)
    }
}
