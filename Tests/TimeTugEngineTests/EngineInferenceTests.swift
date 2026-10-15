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
        let fileAtSecondPass = LockedBox<Bool?>(nil)
        generator.onCall.set { number in
            if number == 21 { fileAtSecondPass.set(fileExists(dedupFile)) }
        }
        let widgetsBefore = h.widgetSnapshots.count

        await h.engine.send(.applySettings { $0.inferenceEnabled = true })

        #expect(generator.calls.get() == 200)
        // The first pass's verdicts were published but not yet written when the second pass began.
        #expect(fileAtSecondPass.get() == false)
        #expect(fileExists(dedupFile))
        let state = await h.engine.currentState()
        #expect(state.inferenceStatus == .active(FakeEngineGenerator.onDevice))
        #expect(state.events.count == 21)
        // One widget snapshot for the settings change, one for the re-snapshot with inference on, one per pass.
        await h.waitUntil("the widget snapshots") { h.widgetSnapshots.count >= widgetsBefore + 12 }
        await h.settle()
        #expect(h.widgetSnapshots.count == widgetsBefore + 12)

        // The backstop stopped at 10 passes; nothing retries on its own. The next refresh judges the rest.
        await h.settle()
        #expect(generator.calls.get() == 200)
        await h.engine.send(.refresh)
        await h.waitUntil("the remaining pairs") { generator.calls.get() == 210 }
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
