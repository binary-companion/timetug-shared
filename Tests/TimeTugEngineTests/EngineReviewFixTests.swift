import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Whole-engine review fixes: invalid account commands, the add-account choices, teardown without `.stop`, and
/// the system calendar switch under a concurrent external change.
@Suite struct EngineReviewFixTests {
    private func started(_ h: EngineHarness) async {
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok }
        await h.settle()
    }

    private func accountDiagnostics(_ h: EngineHarness) -> [EngineDiagnostic] {
        h.emitted.get().compactMap { if case .diagnostic(let d) = $0, d.category == "accounts" { d } else { nil } }
    }

    // 1
    @Test func invalidAccountCommandsSetAnErrorAndEmitADiagnostic() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)])
        await started(h)

        await h.engine.send(.addAccount(kindID: "nope"))
        var state = await h.waitForState { $0.accountStatus.errorMessage != nil }
        #expect(state.accountStatus.errorMessage == "Unknown account type.")
        await h.waitUntil("the add diagnostic") { accountDiagnostics(h).count == 1 }

        await h.engine.send(.reauthorizeAccount(connectionID: "missing"))
        state = await h.engine.currentState()
        #expect(state.accountStatus.errorMessage == "That account no longer exists.")
        await h.waitUntil("the reauthorize diagnostic") { accountDiagnostics(h).count == 2 }

        await h.engine.send(.removeAccount(connectionID: "gone"))
        state = await h.engine.currentState()
        #expect(state.accountStatus.errorMessage == "That account no longer exists.")
        await h.waitUntil("the remove diagnostic") { accountDiagnostics(h).count == 3 }
        #expect(state.accounts.map(\.connectionID) == ["c1"])
        #expect(accountDiagnostics(h).allSatisfy { $0.level == .warning })
    }

    // 2
    @Test func availableAccountKindsListTheChoicesWithCalDAVLastAndUnconfiguredMarked() async throws {
        let caldav = FakeConnectorKind(id: "caldav", displayName: "Other CalDAV", authorization: .password(fields: []))
        let icloud = FakeConnectorKind(id: "icloud", displayName: "iCloud", authorization: .password(fields: []))
        let system = FakeConnectorKind(id: "eventkit", displayName: "Apple Calendar", authorization: .system)
        let microsoft = FakeConnectorKind(id: "microsoft", displayName: "Microsoft")
        let h = try await EngineHarness(
            events: [.sample(id: "e1", startsIn: 3600)], extraKinds: [caldav, icloud, system, microsoft],
            unconfiguredKindIDs: ["google", "microsoft"])
        await started(h)
        let kinds = await h.engine.currentState().availableAccountKinds
        #expect(kinds == [
            AccountKindInfo(id: "fake", displayName: "Fake", authorization: .oauth, isConfigured: true),
            AccountKindInfo(id: "google", displayName: "Google", authorization: .oauth, isConfigured: false),
            AccountKindInfo(id: "icloud", displayName: "iCloud", authorization: .password, isConfigured: true),
            AccountKindInfo(id: "microsoft", displayName: "Microsoft", authorization: .oauth, isConfigured: false),
            AccountKindInfo(id: "caldav", displayName: "Other CalDAV", authorization: .password, isConfigured: true),
        ])
    }

    // 3
    @Test func droppingTheEngineWithoutStopEndsEveryTask() async throws {
        weak var weakEngine: Engine?
        let clock: ManualClock
        let source: ScriptedSource
        do {
            let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
            await started(h)
            await h.engine.send(.setCountdownVisible(true))
            clock = h.clock
            source = h.source()
            #expect(clock.pendingSleeps > 0)
            #expect(!source.listenerEnded)
            weakEngine = h.engine
        }
        await poll("the engine and its tasks to end") {
            weakEngine == nil && clock.pendingSleeps == 0 && source.listenerEnded
        }
        #expect(weakEngine == nil)
        #expect(clock.pendingSleeps == 0)
        #expect(source.listenerEnded)
    }

    // 4
    @Test func theLastExternalSystemCalendarValueWinsOverAnInFlightChange() async throws {
        let gate = Gate()
        let h = try await EngineHarness(
            events: [.sample(id: "e1", startsIn: 3600)], systemSource: FakeSource(id: "eventkit"), systemAccessGate: gate)
        await started(h)
        let savedBefore = h.settings.saved.count

        // Turning it on waits in the host's access request...
        let turnOn = Task { await h.engine.send(.applySettings { $0.systemCalendarEnabled = true }) }
        await h.waitUntil("the access request") { await gate.isWaiting }
        // ...while another process turns it off again.
        var external = h.settings.load()
        external.systemCalendarEnabled = false
        h.settings.save(external)
        await h.engine.send(.settingsChangedExternally)
        await gate.open()
        await turnOn.value
        await h.settle()

        // Only the engine wrote (the change itself); the account manager never wrote the stale `true` back.
        #expect(h.settings.saved.count == savedBefore + 2)
        #expect(h.settings.load().systemCalendarEnabled == false)
        let state = await h.engine.currentState()
        #expect(state.settings.systemCalendarEnabled == false)
        #expect(state.sourceNames["eventkit"] == nil)
    }
}
