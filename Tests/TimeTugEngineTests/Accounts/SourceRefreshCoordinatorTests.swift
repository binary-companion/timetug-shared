import TimeTugCore
import Testing
@testable import TimeTugEngine

@Suite struct SourceRefreshCoordinatorTests {
    @Test func burstDuringSuspendedRefreshRunsOneFollowUpWithAllDirtySources() async {
        let calls = LockedBox<[Set<String>?]>([])
        let gate = Gate()
        let activity = EngineActivity()
        let coordinator = SourceRefreshCoordinator(activity: activity) { ids in
            let count = calls.mutate { $0.append(ids); return $0.count }
            if count == 1 { await gate.wait() }
        }
        await coordinator.signal(.eventsChanged(sourceID: "a", calendarIDs: ["one"]))
        // The first pass is held at the gate; every later signal is awaited, so all of them arrive while it is held.
        await poll("the first pass at the gate") { await gate.isWaiting }
        #expect(calls.get() == [["a"]])
        for _ in 0..<100 { await coordinator.signal(.eventsChanged(sourceID: "a", calendarIDs: ["one"])) }
        await coordinator.signal(.sourceFailed(sourceID: "b"))
        await gate.open()
        // Wait for the worker to finish (it ends after its last pass), so a stray third pass would show up below.
        await poll("the worker to finish") { activity.isIdle && calls.get().count >= 2 }
        #expect(calls.get() == [["a"], ["a", "b"]])
    }

    @Test func unknownCalendarScopeRequestsFullRefresh() async {
        let calls = LockedBox<[Set<String>?]>([])
        let activity = EngineActivity()
        let coordinator = SourceRefreshCoordinator(activity: activity) { ids in calls.mutate { $0.append(ids) } }
        await coordinator.signal(.eventsChanged(sourceID: "a", calendarIDs: nil))
        await poll("the worker to finish") { activity.isIdle && !calls.get().isEmpty }
        let all = calls.get()
        #expect(all.count == 1)
        #expect(all.first! == nil)
    }
}
