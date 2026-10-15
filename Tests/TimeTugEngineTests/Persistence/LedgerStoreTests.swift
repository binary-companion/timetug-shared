import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct LedgerStoreTests {
    private func event(_ id: String = "e1") -> TimeTugCalendarEvent {
        let start = Date(timeIntervalSince1970: 1_800_000_000)
        return TimeTugCalendarEvent(
            event: CalendarCore.CalendarEvent(
                eventID: id, calendarID: "c", title: "Standup", start: start, end: start.addingTimeInterval(1800)),
            sourceID: "s")
    }

    private func store(_ dir: URL, _ events: LockedBox<[TimeTugEngine.DiagnosticEvent]>? = nil) -> LedgerStore {
        LedgerStore(directory: dir, diagnostics: { e in events?.mutate { $0.append(e) } })
    }

    @Test func fileNameIsFixed() {
        #expect(LedgerStore.fileName == "takeover-ledger.json")
    }

    @Test func missingFileLoadsEmptyLedger() throws {
        #expect(store(try TempDir.make()).load() == TakeoverLedger())
    }

    @Test func corruptFileLoadsEmptyLedger() throws {
        let dir = try TempDir.make()
        try Data("not json {".utf8).write(to: dir.appendingPathComponent("takeover-ledger.json"))
        #expect(store(dir).load() == TakeoverLedger())
    }

    @Test func loadsLedgerWrittenByTheMacApp() throws {
        let dir = try TempDir.make(copying: "takeover-ledger.json")
        let ledger = LedgerStore(directory: dir, diagnostics: { _ in }).load()
        // One fired and one snoozed event, each under an id key and a content key.
        #expect(ledger.keyCount == 4)
    }

    @Test func corruptLedgerLoadsEmptyAndEmitsDiagnostic() throws {
        let dir = try TempDir.make()
        try Data("{not json".utf8).write(to: dir.appendingPathComponent("takeover-ledger.json"))
        let events = LockedBox<[TimeTugEngine.DiagnosticEvent]>([])
        let ledger = LedgerStore(directory: dir, diagnostics: { e in events.mutate { $0.append(e) } }).load()
        #expect(ledger.keyCount == 0)
        #expect(events.get().contains { $0.level == .warning && $0.category == "ledger" })
    }

    @Test func roundTripCreatesDirectoryAndKeepsFiredEventFired() throws {
        let dir = try TempDir.make().appendingPathComponent("nested")
        var ledger = TakeoverLedger()
        ledger.markFired(event(), now: Date(timeIntervalSince1970: 1_799_999_000))
        #expect(store(dir).save(ledger, now: Date(timeIntervalSince1970: 1_799_999_500)))
        #expect(FileManager.default.fileExists(atPath: dir.appendingPathComponent("takeover-ledger.json").path))

        let loaded = store(dir).load()
        #expect(loaded == ledger)
        #expect(loaded.hasFired(event()))
        #expect(loaded.hasFired(event("resynced-id")))
    }

    @Test func saveFailureReturnsFalseAndEmitsError() throws {
        // A path under a regular file cannot be created.
        let file = try TempDir.make().appendingPathComponent("plain")
        FileManager.default.createFile(atPath: file.path, contents: Data())
        let events = LockedBox<[TimeTugEngine.DiagnosticEvent]>([])
        let ok = store(file.appendingPathComponent("x"), events).save(TakeoverLedger(), now: Date())
        #expect(!ok)
        #expect(events.get().contains { $0.level == .error && $0.category == "ledger" })
    }

    @Test func savePrunesExpiredEntriesBeforeWriting() throws {
        let dir = try TempDir.make()
        var ledger = TakeoverLedger()
        ledger.markFired(event(), now: Date(timeIntervalSince1970: 1_799_999_000))
        let afterEnd = Date(timeIntervalSince1970: 1_800_000_000 + 1800 + 1)
        #expect(store(dir).save(ledger, now: afterEnd))
        #expect(store(dir).load() == TakeoverLedger())
    }

    @Test func loadOfFileWithExpiredEntriesIsPrunedByLaunchPrune() throws {
        let dir = try TempDir.make()
        var ledger = TakeoverLedger()
        ledger.markFired(event(), now: Date(timeIntervalSince1970: 1_799_999_000))
        #expect(store(dir).save(ledger, now: Date(timeIntervalSince1970: 1_799_999_500)))

        var loaded = store(dir).load()
        #expect(loaded.hasFired(event()))
        let pruned = loaded.prune(now: Date(timeIntervalSince1970: 1_800_000_000 + 1800 + 1))
        #expect(pruned)
        #expect(loaded == TakeoverLedger())
    }
}
