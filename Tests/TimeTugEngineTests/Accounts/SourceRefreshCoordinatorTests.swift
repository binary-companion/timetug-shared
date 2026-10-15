import TimeTugCore
import Testing
@testable import TimeTugEngine

@Suite struct SourceRefreshCoordinatorTests {
    @Test func burstDuringSuspendedRefreshRunsOneFollowUpWithAllDirtySources() async {
        let calls = LockedBox<[Set<String>?]>([])
        let gate = Gate()
        let coordinator = SourceRefreshCoordinator { ids in
            let count = calls.mutate { $0.append(ids); return $0.count }
            if count == 1 { await gate.wait() }
        }
        await coordinator.signal(.eventsChanged(sourceID: "a", calendarIDs: ["one"]))
        for _ in 0..<500 where calls.get().isEmpty { await Task.yield(); try? await Task.sleep(for: .milliseconds(2)) }
        #expect(calls.get() == [["a"]])
        for _ in 0..<100 { await coordinator.signal(.eventsChanged(sourceID: "a", calendarIDs: ["one"])) }
        await coordinator.signal(.sourceFailed(sourceID: "b"))
        await gate.open()
        for _ in 0..<500 where calls.get().count < 2 { await Task.yield(); try? await Task.sleep(for: .milliseconds(2)) }
        #expect(calls.get() == [["a"], ["a", "b"]])
    }

    @Test func unknownCalendarScopeRequestsFullRefresh() async {
        let calls = LockedBox<[Set<String>?]>([])
        let coordinator = SourceRefreshCoordinator { ids in calls.mutate { $0.append(ids) } }
        await coordinator.signal(.eventsChanged(sourceID: "a", calendarIDs: nil))
        for _ in 0..<500 where calls.get().isEmpty { await Task.yield(); try? await Task.sleep(for: .milliseconds(2)) }
        let all = calls.get()
        #expect(all.count == 1)
        #expect(all.first! == nil)
    }
}
