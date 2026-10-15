import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct EngineSettingsCommandTests {
    private func started(_ h: EngineHarness, sources: Int = 1) async {
        await h.engine.send(.start)
        await h.waitForState { $0.statuses.count == sources && $0.statuses.values.allSatisfy { $0 == .ok } }
        await h.settle()
    }

    private func expectedWidget(_ state: EngineState, _ h: EngineHarness) -> WidgetSnapshot {
        WidgetSnapshot.make(events: state.events, calendars: state.calendars, settings: state.settings.takeover,
                            now: h.clock.now(), calendar: EngineHarness.utc)
    }

    // Scenario 6
    @Test func applySettingsPersistsThroughTheHostAndEmitsAWidgetSnapshot() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)])
        await started(h)
        let saved = h.settings.saved.count
        let widgets = h.widgetSnapshots.count

        await h.engine.send(.applySettings { $0.takeover.leadTime = 300 })
        let state = await h.engine.currentState()
        #expect(h.settings.saved.count == saved + 1)
        #expect(h.settings.saved.last?.takeover.leadTime == 300)
        #expect(state.settings.takeover.leadTime == 300)
        #expect(state.nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(3300))
        // Scenario 8: a settings change emits a widget snapshot built with the new settings.
        // At least one more widget snapshot; the last one is built with the new settings.
        await h.waitUntil("the widget snapshot") { h.widgetSnapshots.count > widgets && h.widgetSnapshots.last == expectedWidget(state, h) }

        // No change, no write.
        await h.engine.send(.applySettings { $0.takeover.leadTime = 300 })
        #expect(h.settings.saved.count == saved + 1)
    }

    @Test func settingsChangedExternallyRereadsAndRepublishesWithoutWritingBack() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)])
        await started(h)
        var external = h.settings.load()
        external.takeover.leadTime = 300
        external.takeover.skipAllDayEvents.toggle()
        h.settings.save(external)
        let saved = h.settings.saved.count
        let before = await h.engine.currentState()
        let widgets = h.widgetSnapshots.count

        await h.engine.send(.settingsChangedExternally)
        let state = await h.engine.currentState()
        #expect(state.settings == external)
        #expect(state.revision > before.revision)
        #expect(state.nextTakeover?.fireAt == EngineHarness.epoch.addingTimeInterval(3300))
        #expect(h.settings.saved.count == saved)
        await h.waitUntil("the widget snapshot") { h.widgetSnapshots.count > widgets && h.widgetSnapshots.last == expectedWidget(state, h) }

        // Nothing changed since: no publish.
        await h.engine.send(.settingsChangedExternally)
        #expect(await h.engine.currentState().revision == state.revision)
        #expect(h.settings.saved.count == saved)
    }

    @Test func inferenceTurnedOnExternallyResolvesPendingPairs() async throws {
        let generator = FakeEngineGenerator(answer: "same")
        let h = try await EngineHarness(
            accounts: ["c1": [.sample(id: "a", startsIn: 3600)], "c2": [.sample(id: "b", startsIn: 3600)]],
            textGenerator: generator)
        await started(h, sources: 2)
        #expect(await h.engine.currentState().events.count == 2)
        var external = h.settings.load()
        external.inferenceEnabled = true
        h.settings.save(external)
        let saved = h.settings.saved.count

        await h.engine.send(.settingsChangedExternally)
        let state = await h.engine.currentState()
        #expect(state.inferenceStatus == .active(FakeEngineGenerator.onDevice))
        #expect(state.events.count == 1)
        #expect(generator.calls.get() == 1)
        #expect(h.settings.saved.count == saved)
    }

    @Test func theSystemCalendarSwitchGoesThroughTheAccountManager() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)], systemSource: FakeSource(id: "eventkit"))
        await started(h)
        #expect(await h.engine.currentState().sourceNames["eventkit"] == nil)
        #expect(h.systemAccessRequests.get() == 0)

        await h.engine.send(.applySettings { $0.systemCalendarEnabled = true })
        var state = await h.engine.currentState()
        #expect(state.sourceNames["eventkit"] != nil)
        #expect(state.settings.systemCalendarEnabled)
        #expect(h.settings.saved.last?.systemCalendarEnabled == true)
        #expect(h.systemAccessRequests.get() == 1)

        await h.engine.send(.applySettings { $0.systemCalendarEnabled = false })
        state = await h.engine.currentState()
        #expect(state.sourceNames["eventkit"] == nil)
        #expect(h.systemAccessRequests.get() == 1)
    }

    @Test func withoutAHostSystemSourceTheSwitchIsStoredButBuildsNothing() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)])
        await started(h)
        await h.engine.send(.applySettings { $0.systemCalendarEnabled = true })
        let state = await h.engine.currentState()
        #expect(state.settings.systemCalendarEnabled)
        #expect(state.sourceNames.keys.sorted() == ["fake-c1"])
    }

    @Test func signInProgressIsPublishedWhileItRuns() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)])
        await started(h)
        let gate = Gate()
        h.kind.authorizeGate = gate
        let signIn = Task { await h.engine.send(.addAccount(kindID: "fake")) }
        let working = await h.waitForState { $0.accountStatus.isWorking }
        #expect(working.accountStatus.isWorking)
        await gate.open()
        await signIn.value
        let done = await h.waitForState { !$0.accountStatus.isWorking }
        #expect(done.revision > working.revision)
        #expect(done.accounts.count == 2)
    }
}
