import Foundation
import Testing
@testable import TimeTugEngine

@Suite struct EngineClockTests {
    @Test func manualClockWakesSleepersOnlyWhenDeadlinePasses() async throws {
        let clock = ManualClock(start: Date(timeIntervalSince1970: 1_000))
        let woke = LockedBox(false)
        let task = Task {
            try await clock.sleep(until: Date(timeIntervalSince1970: 1_060))
            woke.set(true)
        }
        await clock.waitForSleepers(1)
        await clock.advance(by: 30)
        #expect(woke.get() == false)
        await clock.advance(by: 30)
        try await task.value
        #expect(woke.get() == true)
        #expect(clock.now() == Date(timeIntervalSince1970: 1_060))
    }

    @Test func manualClockReturnsImmediatelyForPastDeadline() async throws {
        let clock = ManualClock(start: Date(timeIntervalSince1970: 1_000))
        try await clock.sleep(until: Date(timeIntervalSince1970: 900))
    }

    @Test func cancelledSleepThrows() async {
        let clock = ManualClock(start: Date(timeIntervalSince1970: 0))
        let task = Task { try await clock.sleep(until: Date(timeIntervalSince1970: 100)) }
        await clock.waitForSleepers(1)
        task.cancel()
        await #expect(throws: CancellationError.self) { try await task.value }
    }

    @Test func systemClockSleepsUntilDeadline() async throws {
        let clock = SystemClock()
        let start = Date()
        try await clock.sleep(until: start.addingTimeInterval(0.05))
        #expect(Date().timeIntervalSince(start) >= 0.04)
    }
}
