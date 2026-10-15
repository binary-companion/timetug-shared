import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct DedupStateStoreTests {
    private func store(_ dir: URL, _ events: LockedBox<[EngineDiagnostic]>? = nil) -> DedupStateStore {
        DedupStateStore(directory: dir, diagnostics: { e in events?.mutate { $0.append(e) } })
    }

    @Test func fileNameIsFixed() {
        #expect(DedupStateStore.fileName == "dedup-state.json")
    }

    @Test func missingFileLoadsEmptyState() throws {
        #expect(store(try TempDir.make()).load() == DedupState())
    }

    @Test func roundTripsLessonsAndVerdicts() throws {
        let s = store(try TempDir.make().appendingPathComponent("nested"))
        var state = DedupState()
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        state.lessons.record(
            MergedMember(title: "A", calendarKey: "s/x", contentKey: "k1", details: "bare", start: Date(timeIntervalSince1970: 0), end: Date(timeIntervalSince1970: 3600)),
            MergedMember(title: "B", calendarKey: "s/y", contentKey: "k2", details: "bare", start: Date(timeIntervalSince1970: 0), end: Date(timeIntervalSince1970: 3600)),
            decision: .same, now: now)
        state.verdicts.store(
            AdjudicationVerdict(requestID: "r", answer: .same),
            engine: EngineInfo(id: "e", displayName: "E", isOnDevice: true),
            end: now.addingTimeInterval(3600), now: now)
        #expect(s.save(state))
        #expect(s.load() == state)
    }

    @Test func loadsStateWrittenByTheMacApp() throws {
        let dir = try TempDir.make(copying: "dedup-state.json")
        let state = store(dir).load()
        #expect(state != DedupState())
    }

    @Test func corruptFileLoadsEmptyStateAndEmitsDiagnostic() throws {
        let dir = try TempDir.make()
        try Data("not json".utf8).write(to: dir.appendingPathComponent("dedup-state.json"))
        let events = LockedBox<[EngineDiagnostic]>([])
        #expect(store(dir, events).load() == DedupState())
        #expect(events.get().contains { $0.level == .warning && $0.category == "dedup" })
    }

    @Test func saveFailureReturnsFalseAndEmitsError() throws {
        let file = try TempDir.make().appendingPathComponent("plain")
        FileManager.default.createFile(atPath: file.path, contents: Data())
        let events = LockedBox<[EngineDiagnostic]>([])
        #expect(!store(file.appendingPathComponent("x"), events).save(DedupState()))
        #expect(events.get().contains { $0.level == .error && $0.category == "dedup" })
    }
}
