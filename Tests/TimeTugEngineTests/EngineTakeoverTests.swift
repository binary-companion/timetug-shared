import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Takeover scheduling, the fire guard, snooze, join, dismiss and the test tug. `ManualClock` drives every timer.
@Suite struct EngineTakeoverTests {
    // MARK: Helpers

    /// Starts the engine and waits until the first refresh has published the source as healthy.
    private func started(_ h: EngineHarness) async {
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok }
    }

    private func event(_ h: EngineHarness, _ sourceEventID: String) async -> TimeTugCalendarEvent? {
        await h.engine.currentState().events.first { $0.sourceEventID == sourceEventID }
    }

    private func requests(_ h: EngineHarness) -> [TakeoverRequest] {
        h.emitted.get().compactMap { if case .takeoverDue(let request, _) = $0 { request } else { nil } }
    }

    private func withdrawnIDs(_ h: EngineHarness) -> [String] {
        h.emitted.get().compactMap { if case .takeoverWithdrawn(let id) = $0 { id } else { nil } }
    }

    private func joinURLs(_ h: EngineHarness) -> [URL] {
        h.emitted.get().compactMap { if case .joinRequested(let url) = $0 { url } else { nil } }
    }

    private func diagnosticMessages(_ h: EngineHarness) -> [String] { h.diagnostics.get().map(\.message) }

    /// Diagnostics never carry calendar content: every sample title starts with "Meeting ".
    private func expectNoTitlesInDiagnostics(_ h: EngineHarness, sourceLocation: SourceLocation = #_sourceLocation) {
        #expect(!diagnosticMessages(h).contains { $0.contains("Meeting ") }, sourceLocation: sourceLocation)
    }

    private func persistedLedger(_ h: EngineHarness) -> TakeoverLedger {
        LedgerStore(directory: h.directory, diagnostics: { _ in }).load()
    }

    // MARK: 1. Lead time

    @Test func fireAtLeadTimeEmitsOnceAndPersistsTheLedger() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        let armed = await h.engine.currentState().nextTakeover
        #expect(armed?.event.sourceEventID == "e1")
        #expect(armed?.fireAt == EngineHarness.epoch.addingTimeInterval(540))

        await h.clock.advance(by: 539)
        await h.settle()
        #expect(h.takeoverDueIDs.isEmpty)

        await h.clock.advance(by: 1)
        await h.waitUntil("the takeover") { h.takeoverDueIDs == ["e1"] }
        await h.settle()
        #expect(h.takeoverDueIDs == ["e1"])
        let e1 = try #require(await event(h, "e1"))
        let state = await h.engine.currentState()
        #expect(state.ledger.hasFired(e1))
        #expect(state.nextTakeover == nil)
        #expect(persistedLedger(h).hasFired(e1))
        #expect(requests(h).first?.hasStarted == false)
        #expect(diagnosticMessages(h).contains { $0.contains("lead time") })
        expectNoTitlesInDiagnostics(h)
    }

    // MARK: 2. Re-check before firing

    @Test func eventEditedOutBeforeTheTimerNeverTakesOver() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        let e1 = try #require(await event(h, "e1"))
        h.source().setEvents([])
        await h.engine.send(.refresh)
        await h.waitForState { $0.events.isEmpty }
        await h.waitUntil("the withdrawal") { withdrawnIDs(h) == [e1.id] }
        await h.clock.advance(by: 600)
        await h.settle()
        #expect(h.takeoverDueIDs.isEmpty)
    }

    @Test func declinedBeforeTheTimerNeverTakesOver() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        var declined = CalendarCore.CalendarEvent.sample(id: "e1", startsIn: 600)
        declined.participation = .invited(.declined)
        h.source().setEvents([declined])
        await h.engine.send(.refresh)
        await h.waitForState { $0.events.first?.responseStatus == .declined }
        await h.clock.advance(by: 600)
        await h.settle()
        #expect(h.takeoverDueIDs.isEmpty)
        #expect(await h.engine.currentState().nextTakeover == nil)
    }

    @Test func optedOutBeforeTheTimerNeverTakesOver() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        await h.engine.send(.applySettings { $0.takeover.takeoverCalendarKeys = [] })
        #expect(await h.engine.currentState().nextTakeover == nil)
        await h.clock.advance(by: 600)
        await h.settle()
        #expect(h.takeoverDueIDs.isEmpty)
    }

    // MARK: 3. Review Focus 3: a second takeover waits while one is visible

    @Test func secondTakeoverWaitsWhileOneIsVisibleAndFiresOnClose() async throws {
        let h = try await EngineHarness(events: [.sample(id: "a", startsIn: 600), .sample(id: "b", startsIn: 600)])
        await started(h)
        await h.clock.advance(by: 540)
        await h.waitUntil("the first takeover") { h.takeoverDueIDs.count == 1 }
        let first = try #require(requests(h).first?.event)
        await h.engine.send(.takeoverPresented(eventID: first.id, onScreen: true))
        await h.clock.advance(by: 30)
        await h.settle()

        let secondID = first.sourceEventID == "a" ? "b" : "a"
        let second = try #require(await event(h, secondID))
        var state = await h.engine.currentState()
        #expect(h.takeoverDueIDs.count == 1)
        #expect(state.takeoverVisible)
        #expect(state.nextTakeover == nil)
        #expect(!state.ledger.hasFired(second))
        #expect(!persistedLedger(h).hasFired(second))

        await h.engine.send(.takeoverClosed(eventID: first.id))
        await h.waitUntil("the second takeover") { h.takeoverDueIDs.count == 2 }
        await h.settle()
        #expect(h.takeoverDueIDs == [first.sourceEventID, secondID])
        state = await h.engine.currentState()
        #expect(state.ledger.hasFired(second))
        #expect(!state.takeoverVisible)
    }

    @Test func aToastFallbackAlsoHoldsTheSecondTakeover() async throws {
        let h = try await EngineHarness(events: [.sample(id: "a", startsIn: 600), .sample(id: "b", startsIn: 900)])
        await started(h)
        await h.clock.advance(by: 540)
        await h.waitUntil("the first takeover") { h.takeoverDueIDs == ["a"] }
        let a = try #require(await event(h, "a"))
        await h.engine.send(.takeoverPresented(eventID: a.id, onScreen: false))
        await h.clock.advance(by: 300)
        await h.settle()
        #expect(h.takeoverDueIDs == ["a"])
        #expect(await h.engine.currentState().takeoverVisible)
        await h.engine.send(.dismiss(eventID: a.id))
        await h.waitUntil("the second takeover") { h.takeoverDueIDs == ["a", "b"] }
    }

    @Test func aCloseForAnotherEventDoesNotReleaseTheGuard() async throws {
        let h = try await EngineHarness(events: [.sample(id: "a", startsIn: 600), .sample(id: "b", startsIn: 600)])
        await started(h)
        await h.clock.advance(by: 540)
        await h.waitUntil("the first takeover") { h.takeoverDueIDs.count == 1 }
        let first = try #require(requests(h).first?.event)
        await h.engine.send(.takeoverPresented(eventID: first.id, onScreen: true))
        await h.engine.send(.takeoverClosed(eventID: "stale/other/0"))
        await h.settle()
        #expect(h.takeoverDueIDs.count == 1)
        #expect(await h.engine.currentState().takeoverVisible)
    }

    // MARK: 4. Review Focus 4: wake after sleeping past a meeting's start

    @Test func wakeThreeHoursLatePresentsOnceAndNeverForAnEndedEvent() async throws {
        let h = try await EngineHarness(events: [
            .sample(id: "ended", startsIn: 600, duration: 1800),
            .sample(id: "long", startsIn: 600, duration: 5 * 3600),
        ])
        await started(h)
        await h.clock.advance(by: 3 * 3600)
        await h.waitUntil("the late takeover") { h.takeoverDueIDs == ["long"] }
        // The host's wake handling refreshes; doing it twice must not present again.
        await h.engine.send(.refresh)
        await h.engine.send(.refresh)
        await h.clock.advance(by: 0)
        await h.settle()
        #expect(h.takeoverDueIDs == ["long"])
        #expect(requests(h).first?.hasStarted == true)
        #expect(diagnosticMessages(h).contains { $0.contains("late after wake") })
        expectNoTitlesInDiagnostics(h)
    }

    // MARK: 5. Snooze

    @Test func snoozeRearmsAndFiresAgainWhenItExpires() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600, duration: 400)])
        await started(h)
        await h.clock.advance(by: 540)
        await h.waitUntil("the takeover") { h.takeoverDueIDs == ["e1"] }
        let request = try #require(requests(h).first)
        // The meeting ends at +1000; at +540 a 600-second snooze would outlast it.
        #expect(request.snoozeOptions == [60, 300])
        await h.engine.send(.takeoverPresented(eventID: request.event.id, onScreen: true))

        await h.engine.send(.snooze(eventID: request.event.id, seconds: 300))
        let e1 = try #require(await event(h, "e1"))
        var state = await h.engine.currentState()
        #expect(state.ledger.isSnoozed(e1))
        #expect(persistedLedger(h).isSnoozed(e1))
        #expect(!state.takeoverVisible)
        #expect(state.nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(840))

        await h.clock.advance(by: 299)
        await h.settle()
        #expect(h.takeoverDueIDs == ["e1"])
        await h.clock.advance(by: 1)
        await h.waitUntil("the snoozed takeover") { h.takeoverDueIDs == ["e1", "e1"] }
        state = await h.engine.currentState()
        #expect(state.ledger.hasFired(e1))
        #expect(diagnosticMessages(h).contains { $0.contains("snooze expired") })
    }

    // MARK: 6. Join and dismiss

    @Test func joinRequestsTheURLAndCloses() async throws {
        var meeting = CalendarCore.CalendarEvent.sample(id: "e1", startsIn: 600)
        let url = URL(string: "https://meet.google.com/abc-defg-hij")!
        meeting.conferences = [ConferenceInfo(url: url, provider: .meet)]
        let h = try await EngineHarness(events: [meeting, .sample(id: "later", startsIn: 3600)])
        await started(h)
        await h.clock.advance(by: 540)
        await h.waitUntil("the takeover") { h.takeoverDueIDs == ["e1"] }
        let request = try #require(requests(h).first)
        #expect(request.joinURL == url)
        await h.engine.send(.takeoverPresented(eventID: request.event.id, onScreen: true))
        await h.engine.send(.join(eventID: request.event.id))
        await h.waitUntil("the join request") { joinURLs(h) == [url] }
        let state = await h.engine.currentState()
        #expect(!state.takeoverVisible)
        #expect(state.nextTakeover?.event.sourceEventID == "later")
    }

    @Test func dismissClosesWithoutJoining() async throws {
        var meeting = CalendarCore.CalendarEvent.sample(id: "e1", startsIn: 600)
        meeting.conferences = [ConferenceInfo(url: URL(string: "https://meet.google.com/abc-defg-hij")!, provider: .meet)]
        let h = try await EngineHarness(events: [meeting, .sample(id: "later", startsIn: 3600)])
        await started(h)
        await h.clock.advance(by: 540)
        await h.waitUntil("the takeover") { h.takeoverDueIDs == ["e1"] }
        let id = try #require(requests(h).first?.event.id)
        await h.engine.send(.takeoverPresented(eventID: id, onScreen: true))
        await h.engine.send(.dismiss(eventID: id))
        await h.settle()
        #expect(joinURLs(h).isEmpty)
        let state = await h.engine.currentState()
        #expect(!state.takeoverVisible)
        #expect(state.nextTakeover?.event.sourceEventID == "later")
    }

    // MARK: 7. Test tug

    @Test func testTugEmitsTheSampleAndLeavesTheLedgerUntouched() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        let file = h.directory.appendingPathComponent(LedgerStore.fileName)
        let before = try Data(contentsOf: file)
        let ledgerBefore = await h.engine.currentState().ledger

        await h.engine.send(.testTug)
        await h.waitUntil("the test takeover") { requests(h).count == 1 }
        let request = try #require(requests(h).first)
        #expect(request.event.sourceEventID == "test")
        #expect(request.event.calendarID == "test")
        #expect(request.event.sourceID == "test")
        #expect(request.event.start == h.clock.now().addingTimeInterval(60))
        await h.engine.send(.takeoverPresented(eventID: request.event.id, onScreen: true))
        await h.engine.send(.snooze(eventID: request.event.id, seconds: 300))
        await h.engine.send(.testTug)
        await h.waitUntil("the second test takeover") { requests(h).count == 2 }
        await h.engine.send(.join(eventID: request.event.id))
        await h.settle()

        #expect(joinURLs(h).isEmpty)
        #expect(try Data(contentsOf: file) == before)
        #expect(await h.engine.currentState().ledger == ledgerBefore)
        #expect(await h.engine.currentState().nextTakeover?.event.sourceEventID == "e1")
    }

    // MARK: 8. Settings change

    @Test func leadTimeZeroRearmsAtTheStart() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        await h.engine.send(.applySettings { $0.takeover.leadTime = 0 })
        let state = await h.engine.currentState()
        #expect(state.nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(600))
        #expect(withdrawnIDs(h).isEmpty)
        await h.clock.advance(by: 540)
        await h.settle()
        #expect(h.takeoverDueIDs.isEmpty)
        await h.clock.advance(by: 60)
        await h.waitUntil("the takeover at the start") { h.takeoverDueIDs == ["e1"] }
        #expect(requests(h).first?.hasStarted == true)
    }

    @Test func disablingTakeoversWithdrawsThePendingOne() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await started(h)
        let e1 = try #require(await event(h, "e1"))
        await h.engine.send(.applySettings { $0.takeover.enabled = false })
        await h.waitUntil("the withdrawal") { withdrawnIDs(h) == [e1.id] }
        #expect(await h.engine.currentState().nextTakeover == nil)
        await h.clock.advance(by: 600)
        await h.settle()
        #expect(h.takeoverDueIDs.isEmpty)
    }
}
