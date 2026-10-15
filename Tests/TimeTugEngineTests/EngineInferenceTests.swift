import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// One meeting per account, all at the same time with different titles: every pair is a look-alike only a model
/// can decide (different calendars, nothing shared, nothing conflicting).
private func lookAlikes(_ count: Int) -> [String: [CalendarCore.CalendarEvent]] {
    Dictionary(uniqueKeysWithValues: (1...count).map { ("c\($0)", [CalendarCore.CalendarEvent.sample(id: "a\($0)", startsIn: 3600)]) })
}

private func allOK(_ state: EngineState, count: Int) -> Bool {
    state.statuses.count == count && state.statuses.values.allSatisfy { $0 == .ok }
}

private func fileExists(_ url: URL) -> Bool { FileManager.default.fileExists(atPath: url.path) }

/// Records, for every generate call, whether the dedup file existed when it began. The file is written once, at the
/// end of a resolution round, so the first call that sees it is the first call of the next round.
private final class RoundProbe: @unchecked Sendable {
    private let file: URL
    private let seen = LockedBox<[Int: Bool]>([:])
    init(_ file: URL) { self.file = file }
    func record(_ call: Int) { let exists = fileExists(file); seen.mutate { $0[call] = exists } }
    func fileAt(_ call: Int) -> Bool? { seen.get()[call] }
    /// Calls made before the first write, i.e. in the first round that judged anything.
    func callsBeforeFirstWrite() -> Int? {
        let calls = seen.get()
        return calls.keys.sorted().first { calls[$0] == true }.map { $0 - 1 }
    }
}

@Suite struct EngineInferenceTests {
    // Scenario 1 (Review Focus 6)
    @Test func lookAlikePairsResolveInAtMostTenPassesAndDedupIsPersistedOnceAtTheEnd() async throws {
        // 21 look-alikes make 210 pairs; a pass judges at most 20, so 10 passes leave 10 pairs for later.
        let generator = FakeEngineGenerator(answer: "different")
        let h = try await EngineHarness(accounts: lookAlikes(21), textGenerator: generator)
        await h.engine.send(.start)
        await h.waitForState { allOK($0, count: 21) }
        await h.settle()
        #expect(generator.calls.get() == 0)
        #expect(await h.engine.currentState().inferenceStatus == .disabled)

        let dedupFile = h.dedupFile
        let probe = RoundProbe(dedupFile)
        generator.onCall.set { probe.record($0) }
        let widgetsBefore = h.widgetSnapshots.count

        await h.engine.send(.applySettings { $0.inferenceEnabled = true })

        // Round one made exactly 10 passes of 20 calls: the dedup file, written once at the end of a round, did not
        // exist on the second pass or the 200th call, and did on the first call after round one (checked below,
        // once that call has happened). (If an earlier round was still in
        // flight when the setting changed, the engine runs one more round at once; see the next test. The total
        // then reaches 210 here, but round one is still 200 calls.)
        #expect(probe.fileAt(21) == false)
        #expect(probe.fileAt(200) == false)
        #expect([200, 210].contains(generator.calls.get()))
        #expect(fileExists(dedupFile))
        let state = await h.engine.currentState()
        #expect(state.inferenceStatus == .active(FakeEngineGenerator.onDevice))
        #expect(state.events.count == 21)
        // Each pass's verdicts republish: at least one widget snapshot per pass (10), plus the settings change and
        // the re-snapshot with inference on.
        await h.waitUntil("the widget snapshots") { h.widgetSnapshots.count >= widgetsBefore + 12 }

        // Nothing retries on its own; the next refresh judges whatever is left.
        await h.engine.send(.refresh)
        await h.waitUntil("the remaining pairs") { generator.calls.get() == 210 }
        await h.settle()
        #expect(generator.calls.get() == 210)
        #expect(probe.callsBeforeFirstWrite() == 200)
    }

    /// The CI interleaving of the test above, made deterministic: the round started by the launch refresh (inference
    /// still off) is held in flight when inference is turned on. The settings change joins that round and asks for
    /// one more, so round one judges with inference on (10 passes, 200 calls, then one write) and round two judges
    /// the 10 left. Verdict publications never request a round themselves, so it stops there.
    @Test func aSettingChangeDuringAnInFlightRoundRunsExactlyOneMoreRound() async throws {
        let generator = FakeEngineGenerator(answer: "different")
        let gate = Gate()
        // Read 1 is `start`'s own availability check; read 2 is the launch refresh's resolution round.
        generator.availabilityGate.set((read: 2, gate: gate))
        let h = try await EngineHarness(accounts: lookAlikes(21), textGenerator: generator)
        let probe = RoundProbe(h.dedupFile)
        generator.onCall.set { probe.record($0) }
        await h.engine.send(.start)
        await h.waitForState { allOK($0, count: 21) }
        await h.waitUntil("the launch round at the gate") { await gate.isWaiting }
        let widgetsBefore = h.widgetSnapshots.count

        let turnOn = Task { await h.engine.send(.applySettings { $0.inferenceEnabled = true }) }
        // The settings change and the re-snapshot both emitted: the change is now waiting on the held round.
        await h.waitUntil("the settings change to join the round") { h.widgetSnapshots.count >= widgetsBefore + 2 }
        #expect(generator.calls.get() == 0)
        await gate.open()
        await turnOn.value

        #expect(probe.callsBeforeFirstWrite() == 200)
        #expect(generator.calls.get() == 210)
        await h.settle()
        #expect(generator.calls.get() == 210)
    }

    // Scenario 2
    @Test func temporarilyUnavailableKeepsPairsPendingUntilTheNextRefresh() async throws {
        let generator = FakeEngineGenerator(.temporarilyUnavailable(reason: "low power"), answer: "same")
        let h = try await EngineHarness(accounts: lookAlikes(2), textGenerator: generator, inferenceEnabled: true)
        await h.engine.send(.start)
        await h.waitForState { allOK($0, count: 2) }
        await h.settle()
        var state = await h.engine.currentState()
        #expect(generator.calls.get() == 0)
        #expect(state.inferenceStatus == .unavailable(reason: "temporary: low power"))
        #expect(state.events.count == 2)
        #expect(!fileExists(h.dedupFile))

        // Becoming available does not start a retry loop on its own.
        generator.availabilityValue.set(.available(FakeEngineGenerator.onDevice))
        await h.clock.advance(by: 60)
        await h.settle()
        #expect(generator.calls.get() == 0)

        await h.engine.send(.refresh)
        state = await h.waitForState { $0.events.count == 1 }
        #expect(state.inferenceStatus == .active(FakeEngineGenerator.onDevice))
        await h.settle()
        #expect(generator.calls.get() == 1)
    }

    // Scenario 3
    @Test func aThrowingGeneratorRecordsNoVerdictAndRetriesOnlyOnTheNextRefresh() async throws {
        let generator = FakeEngineGenerator(answer: nil)
        let h = try await EngineHarness(accounts: lookAlikes(2), textGenerator: generator, inferenceEnabled: true)
        await h.engine.send(.start)
        await h.waitForState { allOK($0, count: 2) }
        await h.waitUntil("the first judgment") { generator.calls.get() == 1 }
        await h.settle()
        #expect(generator.calls.get() == 1)
        #expect(await h.engine.currentState().events.count == 2)
        let diagnostics = h.diagnostics.get()
        #expect(diagnostics.contains { $0.category == "inference" && $0.message.contains("Failure") })
        #expect(!diagnostics.contains { $0.message.contains("Meeting ") })

        // No timer retries it: once the backoff has passed, still only one call until something refreshes.
        await h.clock.advance(by: 60)
        await h.settle()
        #expect(generator.calls.get() == 1)

        generator.answer.set("same")
        await h.engine.send(.refresh)
        await h.waitForState { $0.events.count == 1 }
        await h.settle()
        #expect(generator.calls.get() == 2)
    }

    // Scenario 4
    @Test func dedupActionsPublishAndPersistOnlyWhenTheStateChanged() async throws {
        let h = try await EngineHarness(events: [
            .sample(id: "a", startsIn: 600), .sample(id: "b", startsIn: 900), .sample(id: "c", startsIn: 7200),
        ])
        await h.engine.send(.start)
        var state = await h.waitForState { $0.events.count == 3 }
        func event(_ id: String) -> TimeTugCalendarEvent { state.events.first { $0.sourceEventID == id }! }
        let file = h.dedupFile
        #expect(!fileExists(file))

        func run(_ command: EngineCommand, persists: Bool, sourceLocation: SourceLocation = #_sourceLocation) async {
            let before = state.revision
            await h.engine.send(command)
            state = await h.engine.currentState()
            #expect(state.revision > before, sourceLocation: sourceLocation)
            #expect(fileExists(file) == persists, sourceLocation: sourceLocation)
            try? FileManager.default.removeItem(at: file)
        }

        await run(.merge(event("a"), event("b")), persists: true)
        #expect(state.events.count == 2)
        let diagnostics = h.diagnostics.get().filter { $0.category == "dedup" }
        #expect(diagnostics.contains { $0.message.contains("2 copies") && $0.message.contains("2 events") })
        #expect(!h.diagnostics.get().contains { $0.message.contains("Meeting ") })

        // Unmerging an event that was never merged teaches nothing.
        await run(.unmerge(event("c")), persists: false)
        #expect(state.events.count == 2)

        let merged = state.events.first { $0.mergedMembers.count == 2 }!
        await run(.separate(merged, members: [merged.mergedMembers[0]]), persists: true)
        #expect(state.events.count == 3)

        await run(.merge(event("a"), event("b")), persists: true)
        let mergedAgain = state.events.first { $0.mergedMembers.count == 2 }!
        await run(.unmerge(mergedAgain), persists: true)
        #expect(state.events.count == 3)

        await run(.forgetCorrections, persists: true)
        // Nothing left to forget.
        await run(.forgetCorrections, persists: false)
    }
}
