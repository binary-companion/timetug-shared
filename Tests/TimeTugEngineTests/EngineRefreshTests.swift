import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct EngineRefreshTests {
    @Test func startPublishesAgendaAndOkStatus() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)])
        await h.engine.send(.start)
        let state = await h.waitForState { $0.statuses["fake-c1"] == .ok }
        #expect(state.agenda.items.count == 1)
        #expect(state.agenda.next?.sourceEventID == "e1")
        #expect(state.revision > 0)
        #expect(state.accounts.map(\.connectionID) == ["c1"])
        // A new subscriber receives the current state first. Status and tick publications may still land after
        // `state` was read, so the first value lies between that state and the engine's current one.
        var states = await h.engine.states.makeAsyncIterator()
        let current = await h.engine.currentState().revision
        let first = try #require(await states.next()?.revision)
        #expect(first >= state.revision && first <= current)
    }

    @Test func sourceSignalRefreshesOnlyThatSourceAndABurstCausesOneFollowUp() async throws {
        let h = try await EngineHarness(accounts: [
            "c1": [.sample(id: "a", startsIn: 600)], "c2": [.sample(id: "b", startsIn: 1200)],
        ])
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok && $0.statuses["fake-c2"] == .ok }
        await h.settle()
        let c1 = h.source("c1"), c2 = h.source("c2")
        let (c1Base, c2Base) = (c1.eventsCalls, c2.eventsCalls)

        c1.signal(.eventsChanged(calendarIDs: ["cal"]))
        await h.settle()
        #expect(c1.eventsCalls == c1Base + 1)
        #expect(c2.eventsCalls == c2Base)

        // A burst of three signals while a refresh is held at the gate runs exactly one follow-up pass. The test
        // releases the gate only once the coordinator has received all three (counted, not yielded for), so the
        // coalescing is deterministic.
        let received = h.engine.activity.signalsReceived
        c1.setGated(true)
        c1.signal(.eventsChanged(calendarIDs: ["cal"]))
        await h.waitUntil("the refresh to reach the gate") { await c1.gate.waiting == 1 }
        c1.setEvents([.sample(id: "a", startsIn: 600), .sample(id: "late", startsIn: 900)])
        for _ in 0..<3 { c1.signal(.eventsChanged(calendarIDs: ["cal"])) }
        await h.waitUntil("the coordinator to receive the burst") { h.engine.activity.signalsReceived == received + 4 }
        c1.setGated(false)
        await c1.gate.releaseAll()
        await h.settle()
        #expect(c1.eventsCalls == c1Base + 3)
        #expect(c2.eventsCalls == c2Base)
        // The follow-up read the data written during the burst.
        #expect(await h.engine.currentState().events.map(\.sourceEventID).contains("late"))
    }

    @Test func periodicRefreshFiresEvery900Seconds() async throws {
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 3600)])
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok }
        // The periodic deadline is fixed when it is armed (start, then after each periodic refresh), so once the
        // engine is quiescent the next one is +900 from the last refresh whether or not its sleep has registered yet.
        await h.settle()
        let base = h.source().eventsCalls
        await h.clock.advance(by: 899)
        await h.settle()
        #expect(h.source().eventsCalls == base)
        await h.clock.advance(by: 1)
        await h.waitUntil("the periodic refresh") { h.source().eventsCalls == base + 1 }
        await h.settle()
        #expect(h.source().eventsCalls == base + 1)
        await h.clock.advance(by: 899)
        await h.settle()
        #expect(h.source().eventsCalls == base + 1)
        await h.clock.advance(by: 1)
        await h.waitUntil("the second periodic refresh") { h.source().eventsCalls == base + 2 }
    }

    @Test func revisionsIncreaseAndAStalePublicationIsDropped() async throws {
        let h = try await EngineHarness(events: [.sample(id: "old", startsIn: 600)])
        let observed = LockedBox<[UInt64]>([])
        let states = await h.engine.states
        let observer = Task { for await state in states { observed.mutate { $0.append(state.revision) } } }
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c1"] == .ok }
        let source = h.source()

        // Refresh A reads the old events and is held; refresh B reads the new events and is held too.
        source.setGated(true)
        let refreshA = Task { await h.engine.send(.refresh) }
        await h.waitUntil("refresh A at the gate") { await source.gate.waiting == 1 }
        source.setEvents([.sample(id: "new", startsIn: 900)])
        let refreshB = Task { await h.engine.send(.refresh) }
        await h.waitUntil("refresh B at the gate") { await source.gate.waiting == 2 }

        // B completes first, then A (out of order).
        await source.gate.release(1)
        await refreshB.value
        let afterB = await h.engine.currentState()
        #expect(afterB.events.map(\.sourceEventID) == ["new"])
        await source.gate.release(0)
        await refreshA.value
        let afterA = await h.engine.currentState()
        #expect(afterA.revision == afterB.revision)
        #expect(afterA.events.map(\.sourceEventID) == ["new"])

        // The stream keeps only the newest state, so a lagging consumer may skip revisions: wait until it has
        // seen the engine's current one, then check that whatever it saw only ever increased.
        let final = await h.engine.currentState().revision
        await h.waitUntil("the observer to reach revision \(final)") { observed.get().last == final }
        observer.cancel()
        let revisions = observed.get()
        #expect(!revisions.isEmpty)
        #expect(zip(revisions, revisions.dropFirst()).allSatisfy { $0 < $1 })
        #expect(final == afterA.revision)
    }

    @Test func launchAcknowledgementMarksMeetingsUnderwayBeforeTheGrace() async throws {
        let h = try await EngineHarness(events: [
            .sample(id: "early", startsIn: -600), .sample(id: "recent", startsIn: -30),
        ])
        await h.engine.send(.start)
        let state = await h.waitForState { $0.statuses["fake-c1"] == .ok }
        let early = try #require(state.events.first { $0.sourceEventID == "early" })
        let recent = try #require(state.events.first { $0.sourceEventID == "recent" })
        #expect(state.ledger.hasFired(early))
        // "recent" is inside the grace, so it is not acknowledged: it is presented late instead. That takeover
        // can land before this state is read, so the outcome is checked rather than the intermediate ledger.
        await h.waitUntil("the late takeover") { h.takeoverDueIDs.contains("recent") }
        await h.settle()
        #expect(h.takeoverDueIDs == ["recent"])
        #expect(await h.engine.currentState().ledger.hasFired(recent))
    }

    @Test func failingSourceKeepsEventsAndRemovedAccountIsNotResurrected() async throws {
        let h = try await EngineHarness(accounts: [
            "c1": [.sample(id: "a", startsIn: 600)], "c2": [.sample(id: "b", startsIn: 1200)],
        ])
        await h.engine.send(.start)
        await h.waitForState { $0.statuses["fake-c2"] == .ok }
        let c2 = h.source("c2")

        c2.setFailure(.network("offline"))
        await h.engine.send(.refresh)
        var state = await h.engine.currentState()
        guard case .failing = state.statuses["fake-c2"] else {
            Issue.record("expected failing, got \(String(describing: state.statuses["fake-c2"]))")
            return
        }
        #expect(state.statuses["fake-c1"] == .ok)
        #expect(Set(state.events.map(\.sourceEventID)) == ["a", "b"])

        // Remove c2 while its read is suspended; the read then succeeds but must not bring its events back.
        c2.setFailure(nil)
        c2.setGated(true)
        let refresh = Task { await h.engine.send(.refresh) }
        await h.waitUntil("the c2 read at the gate") { await c2.gate.waiting == 1 }
        await h.engine.send(.removeAccount(connectionID: "c2"))
        await c2.gate.releaseAll()
        await refresh.value
        await h.settle()
        state = await h.engine.currentState()
        #expect(state.events.map(\.sourceEventID) == ["a"])
        #expect(state.statuses["fake-c2"] == nil)
        #expect(state.statuses["fake-c1"] == .ok)
        #expect(state.accounts.map(\.connectionID) == ["c1"])
    }

    @Test func acknowledgedLedgerIsPersistedAndSurvivesANewEngine() async throws {
        let first = try await EngineHarness(events: [.sample(id: "early", startsIn: -600)])
        await first.engine.send(.start)
        let state = await first.waitForState { $0.statuses["fake-c1"] == .ok }
        let early = try #require(state.events.first { $0.sourceEventID == "early" })
        #expect(LedgerStore(directory: first.directory, diagnostics: { _ in }).load().hasFired(early))
        await first.engine.send(.stop)

        // The second engine's source never reports `.ok`, so it cannot acknowledge anything itself.
        let second = try await EngineHarness(
            events: [.sample(id: "early", startsIn: -600)], failures: ["c1": .network("offline")],
            directory: first.directory)
        await second.engine.send(.start)
        let reloaded = await second.waitForState {
            if case .failing = $0.statuses["fake-c1"] { true } else { false }
        }
        #expect(reloaded.ledger.hasFired(early))
    }
}
