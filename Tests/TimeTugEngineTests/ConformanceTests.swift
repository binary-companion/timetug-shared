import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Scripted timelines run through one engine each. Every test asserts the exact ordered list of `takeoverDue`,
/// `takeoverWithdrawn` and `widgetSnapshot` events (with the clock time each arrived) and the final `EngineState`.
///
/// Determinism: the manual clock moves only when a test advances it, and each advance wakes at most one sleeper
/// that emits events (the periodic refresh at +900 s steps, or one takeover). A refresh publishes once per source
/// as each source answers, so a two-account refresh records two widget snapshots.
@Suite struct ConformanceTests {
    private func started(_ h: EngineHarness, accounts: Int) async {
        await h.engine.send(.start)
        await h.waitForState { $0.statuses.count == accounts && $0.statuses.values.allSatisfy { $0 == .ok } }
    }

    private func event(_ h: EngineHarness, _ sourceEventID: String) async throws -> TimeTugCalendarEvent {
        try #require(await h.engine.currentState().events.first { $0.sourceEventID == sourceEventID })
    }

    private func agenda(_ state: EngineState, _ itemState: DayAgenda.State) -> [String] {
        state.agenda.items.filter { $0.state == itemState }.map(\.event.sourceEventID)
    }

    /// Advances the clock in steps and waits until the timeline holds `count` entries.
    private func advance(_ h: EngineHarness, _ r: TimelineRecorder, by seconds: TimeInterval, expecting count: Int,
                         sourceLocation: SourceLocation = #_sourceLocation) async {
        await h.clock.advance(by: seconds)
        await r.waitFor(count, h, sourceLocation: sourceLocation)
    }

    // MARK: Two accounts, overlapping meetings

    @Test func twoAccountsWithOverlappingMeetings() async throws {
        let h = try await EngineHarness(accounts: [
            "c1": [.sample(id: "a", startsIn: 600)],
            "c2": [.sample(id: "b", startsIn: 1200)],
        ])
        let r = await TimelineRecorder(h)
        await started(h, accounts: 2)
        await r.waitFor(2, h)

        // a fires at its lead time; the host shows it.
        await advance(h, r, by: 540, expecting: 3)
        let a = try await event(h, "a")
        await h.engine.send(.takeoverPresented(eventID: a.id, onScreen: true))

        // The periodic refresh at +900 republishes; a is still on screen.
        await advance(h, r, by: 360, expecting: 5)

        // b's lead time passes while a is on screen: b is held, not fired.
        await h.clock.advance(by: 240)
        await h.settle()
        #expect(r.entries.count == 5)
        #expect(await h.engine.currentState().nextTakeover == nil)

        // Closing a releases b at once; the host snoozes it for 5 minutes, then it fires again.
        await h.engine.send(.takeoverClosed(eventID: a.id))
        await r.waitFor(6, h)
        let b = try await event(h, "b")
        await h.engine.send(.snooze(eventID: b.id, seconds: 300))
        await advance(h, r, by: 300, expecting: 7)
        await h.engine.send(.dismiss(eventID: b.id))

        // The next periodic refresh, at +1800, while both meetings are underway.
        await advance(h, r, by: 360, expecting: 9)

        #expect(r.entries == [
            .widget(at: 0), .widget(at: 0),
            .due("a", at: 540),
            .widget(at: 900), .widget(at: 900),
            .due("b", at: 1140),
            .due("b", at: 1440, late: true),
            .widget(at: 1800), .widget(at: 1800),
        ])
        let state = await h.engine.currentState()
        #expect(state.events.map(\.sourceEventID) == ["a", "b"])
        #expect(state.statuses == ["fake-c1": .ok, "fake-c2": .ok])
        #expect(state.accounts.map(\.connectionID).sorted() == ["c1", "c2"])
        #expect(state.ledger.hasFired(a) && state.ledger.hasFired(b))
        #expect(state.nextTakeover == nil)
        #expect(state.takeoverVisible == false)
        #expect(r.widgets.last?.events.map(\.title) == ["Meeting a", "Meeting b"])
    }

    // MARK: Wake gap

    @Test func aThreeHourWakeGapPresentsTheMeetingUnderwayOnceAndSkipsTheEndedOne() async throws {
        let h = try await EngineHarness(events: [
            .sample(id: "long", startsIn: 1800, duration: 5 * 3600),
            .sample(id: "short", startsIn: 3600),
            .sample(id: "later", startsIn: 4 * 3600),
        ])
        let r = await TimelineRecorder(h)
        await started(h, accounts: 1)
        await r.waitFor(1, h)

        // The machine sleeps for three hours. Every sleeper wakes at once: the overdue takeover fires first,
        // then the held periodic refresh (gated so its publication cannot overtake the takeover) and the host's wake.
        h.source().setGated(true)
        await h.clock.advance(by: 3 * 3600)
        await r.waitFor(2, h)
        await h.waitUntil("the periodic refresh at the gate") { await h.source().gate.waiting == 1 }
        h.source().setGated(false)
        await h.source().gate.releaseAll()
        await r.waitFor(3, h)
        await h.engine.send(.system(.wake))
        await r.waitFor(4, h)

        let long = try await event(h, "long")
        await h.engine.send(.dismiss(eventID: long.id))
        // Periodic refreshes every 900 s from the wake, then "later" at its lead time.
        await advance(h, r, by: 900, expecting: 5)
        await advance(h, r, by: 900, expecting: 6)
        await advance(h, r, by: 900, expecting: 7)
        await advance(h, r, by: 840, expecting: 8)

        #expect(r.entries == [
            .widget(at: 0),
            .due("long", at: 10800, late: true),
            .widget(at: 10800),
            .widget(at: 10800),
            .widget(at: 11700), .widget(at: 12600), .widget(at: 13500),
            .due("later", at: 14340),
        ])
        let state = await h.engine.currentState()
        let short = try await event(h, "short")
        #expect(!state.ledger.hasFired(short))
        #expect(state.ledger.hasFired(long))
        #expect(state.ledger.hasFired(try await event(h, "later")))
        #expect(state.nextTakeover == nil)
        #expect(agenda(state, .current) == ["long"])
    }

    // MARK: Account removal mid-day

    @Test func removingAnAccountMidDayWithdrawsItsArmedMeeting() async throws {
        let h = try await EngineHarness(accounts: [
            "c1": [.sample(id: "a", startsIn: 2400)],
            "c2": [.sample(id: "b", startsIn: 1200)],
        ])
        let r = await TimelineRecorder(h)
        await started(h, accounts: 2)
        await r.waitFor(2, h)
        #expect(await h.engine.currentState().nextTakeover?.event.sourceEventID == "b")

        await h.clock.advance(by: 600)
        await h.settle()
        await h.engine.send(.removeAccount(connectionID: "c2"))
        // The refresh without c2 withdraws b; forgetting c2's calendar selections publishes again.
        await r.waitFor(4, h)

        await advance(h, r, by: 300, expecting: 5)
        await advance(h, r, by: 900, expecting: 6)
        await advance(h, r, by: 540, expecting: 7)

        #expect(r.entries == [
            .widget(at: 0), .widget(at: 0),
            .withdrawn("b", at: 600), .widget(at: 600), .widget(at: 600),
            .widget(at: 900),
            .widget(at: 1800),
            .due("a", at: 2340),
        ])
        let state = await h.engine.currentState()
        #expect(state.accounts.map(\.connectionID) == ["c1"])
        #expect(state.statuses == ["fake-c1": .ok])
        #expect(state.events.map(\.sourceEventID) == ["a"])
        #expect(state.settings.takeover.takeoverCalendarKeys == ["fake-c1/cal"])
        #expect(h.settings.load().takeover.takeoverCalendarKeys == ["fake-c1/cal"])
        #expect(state.ledger.hasFired(try await event(h, "a")))
    }

    // MARK: Settings toggles

    @Test func leadTimeChangeAndDisablingAndReenablingTakeovers() async throws {
        let h = try await EngineHarness(events: [.sample(id: "a", startsIn: 1200)])
        let r = await TimelineRecorder(h)
        await started(h, accounts: 1)
        await r.waitFor(1, h)
        #expect(await h.engine.currentState().nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(1140))

        // A longer lead time re-arms the same meeting earlier (no withdrawal).
        await h.engine.send(.applySettings { $0.takeover.leadTime = 300 })
        await r.waitFor(2, h)
        #expect(await h.engine.currentState().nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(900))

        // Disabling withdraws it; nothing fires at +900, only the periodic refresh publishes.
        await h.engine.send(.applySettings { $0.takeover.enabled = false })
        await r.waitFor(4, h)
        await advance(h, r, by: 900, expecting: 5)

        // Re-enabling inside the lead time fires at once.
        await h.clock.advance(by: 60)
        await h.settle()
        await h.engine.send(.applySettings { $0.takeover.enabled = true })
        await r.waitFor(7, h)

        #expect(r.entries == [
            .widget(at: 0),
            .widget(at: 0),
            .withdrawn("a", at: 0), .widget(at: 0),
            .widget(at: 900),
            .widget(at: 960), .due("a", at: 960),
        ])
        let state = await h.engine.currentState()
        #expect(state.settings.takeover.leadTime == 300)
        #expect(state.settings.takeover.enabled)
        #expect(h.settings.load() == state.settings)
        #expect(h.settings.saved.count == 3)
        #expect(state.ledger.hasFired(try await event(h, "a")))
        #expect(state.nextTakeover == nil)
    }

    // MARK: Inference becomes available

    @Test func inferenceBecomingAvailableMergesTheLookAlikesAndTheMergedMeetingTakesOverOnce() async throws {
        let generator = FakeEngineGenerator(.temporarilyUnavailable(reason: "low power"), answer: "same")
        let h = try await EngineHarness(
            accounts: ["c1": [.sample(id: "a1", startsIn: 3600)], "c2": [.sample(id: "a2", startsIn: 3600)]],
            textGenerator: generator, inferenceEnabled: true)
        let r = await TimelineRecorder(h)
        await started(h, accounts: 2)
        await r.waitFor(2, h)
        #expect(await h.engine.currentState().inferenceStatus == .unavailable(reason: "temporary: low power"))

        // The model comes back; the next refresh re-snapshots with it and its verdict merges the pair.
        await h.clock.advance(by: 600)
        await h.settle()
        generator.availabilityValue.set(.available(FakeEngineGenerator.onDevice))
        await h.engine.send(.refresh)
        await r.waitFor(6, h)
        #expect(generator.calls.get() == 1)

        await advance(h, r, by: 300, expecting: 8)
        await advance(h, r, by: 900, expecting: 10)
        await advance(h, r, by: 900, expecting: 12)
        await advance(h, r, by: 840, expecting: 13)

        let merged = try #require(await h.engine.currentState().events.first)
        #expect(r.entries == [
            .widget(at: 0), .widget(at: 0),
            .widget(at: 600), .widget(at: 600), .widget(at: 600), .widget(at: 600),
            .widget(at: 900), .widget(at: 900),
            .widget(at: 1800), .widget(at: 1800),
            .widget(at: 2700), .widget(at: 2700),
            .due(merged.sourceEventID, at: 3540),
        ])
        let state = await h.engine.currentState()
        #expect(state.events.count == 1)
        #expect(merged.participants.count == 2)
        #expect(state.inferenceStatus == .active(FakeEngineGenerator.onDevice))
        #expect(state.ledger.hasFired(merged))
        #expect(generator.calls.get() == 1)
        #expect(FileManager.default.fileExists(atPath: h.dedupFile.path))
    }

    // MARK: A failing source recovers

    @Test func aFailingSourceRecoversAndItsMeetingTakesOver() async throws {
        let h = try await EngineHarness(
            accounts: ["c1": [.sample(id: "a", startsIn: 1200)], "c2": [.sample(id: "b", startsIn: 600)]],
            failures: ["c2": .network("offline")])
        let r = await TimelineRecorder(h)
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok && $0.statuses["fake-c2"] != nil }
        await r.waitFor(2, h)
        var state = await h.engine.currentState()
        #expect(state.events.map(\.sourceEventID) == ["a"])
        #expect(state.nextTakeover?.event.sourceEventID == "a")

        await h.clock.advance(by: 300)
        await h.settle()
        h.source("c2").setFailure(nil)
        await h.engine.send(.system(.networkReturned))
        await r.waitFor(4, h)
        #expect(await h.engine.currentState().nextTakeover?.event.sourceEventID == "b")

        await advance(h, r, by: 240, expecting: 5)

        #expect(r.entries == [
            .widget(at: 0), .widget(at: 0),
            .widget(at: 300), .widget(at: 300),
            .due("b", at: 540),
        ])
        state = await h.engine.currentState()
        #expect(state.statuses == ["fake-c1": .ok, "fake-c2": .ok])
        #expect(state.events.map(\.sourceEventID) == ["b", "a"])
        #expect(state.ledger.hasFired(try await event(h, "b")))
        #expect(!state.ledger.hasFired(try await event(h, "a")))
        #expect(state.takeoverVisible == false)
    }

    // MARK: The day rolls over

    @Test func theDayRollsOverToTomorrowsMeetings() async throws {
        let midnight: TimeInterval = 12 * 3600
        let h = try await EngineHarness(events: [
            .sample(id: "today", startsIn: 600),
            .sample(id: "tomorrow", startsIn: midnight + 9 * 3600),
        ])
        let r = await TimelineRecorder(h)
        await started(h, accounts: 1)
        await r.waitFor(1, h)
        #expect(agenda(await h.engine.currentState(), .upcoming) == ["today"])

        await advance(h, r, by: 540, expecting: 2)
        let today = try await event(h, "today")
        await h.engine.send(.dismiss(eventID: today.id))

        // Past midnight in one step: the overdue periodic refresh, then the host's day change.
        await advance(h, r, by: midnight - 540, expecting: 3)
        await h.engine.send(.system(.dayChanged))
        await r.waitFor(4, h)

        #expect(r.entries == [
            .widget(at: 0),
            .due("today", at: 540),
            .widget(at: midnight),
            .widget(at: midnight),
        ])
        let state = await h.engine.currentState()
        #expect(state.events.map(\.sourceEventID) == ["tomorrow"])
        #expect(agenda(state, .upcoming) == ["tomorrow"])
        #expect(state.ledger.keyCount == 0)
        #expect(state.nextTakeover?.event.sourceEventID == "tomorrow")
        #expect(state.nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(midnight + 9 * 3600 - 60))
        #expect(r.widgets.last?.events.map(\.title).first == "Meeting tomorrow")
    }
}
