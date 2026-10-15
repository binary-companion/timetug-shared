import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Settings storage over a Mac-format settings file: `load` decodes it (legacy keys included), `save` counts and
/// writes. Stands in for a host that keeps its settings on disk.
private final class LegacyFixtureSettingsStorage: SettingsStorage, @unchecked Sendable {
    let url: URL
    let saves = LockedBox(0)
    init(url: URL) { self.url = url }

    func load() -> EngineSettings {
        let takeover = (try? JSONDecoder().decode(TakeoverSettings.self, from: Data(contentsOf: url))) ?? TakeoverSettings()
        return EngineSettings(takeover: takeover, inferenceEnabled: false, systemCalendarEnabled: false)
    }

    func save(_ settings: EngineSettings) {
        saves.mutate { $0 += 1 }
        try? JSONEncoder().encode(settings.takeover).write(to: url, options: .atomic)
    }
}

/// An engine started over a data directory the Mac app wrote (Review Focus 1 and 2): it reads the Mac files,
/// honours what they say, never rewrites what it only reads, and starts even when a file is corrupt.
@Suite struct LegacyFixtureTests {
    /// The fixtures' events start at 2027-01-15 08:00 UTC (Unix 1_800_000_000) and end 30 minutes later.
    private static let meetingStart = Date(timeIntervalSince1970: 1_800_000_000)

    private static func meeting(_ id: String, title: String, calendar: String = "cal", startsIn: TimeInterval = 0)
        -> CalendarCore.CalendarEvent {
        let start = meetingStart.addingTimeInterval(startsIn)
        return CalendarCore.CalendarEvent(
            eventID: id, calendarID: calendar, title: title, start: start, end: start.addingTimeInterval(1800))
    }

    /// A data directory holding the given fixtures and an `accounts.json` with one "fake" account, "c1".
    private func dataDirectory(copying fixtures: [String]) async throws -> URL {
        let dir = try TempDir.make()
        for name in fixtures {
            try FileManager.default.copyItem(at: Fixtures.url(name), to: dir.appendingPathComponent(name))
        }
        try await FileConnectionStore(url: dir.appendingPathComponent("accounts.json"))
            .add(Connection(kindID: "fake", connectionID: "c1", displayName: "c1@x.test"))
        return dir
    }

    private func bytes(_ dir: URL, _ name: String) throws -> Data {
        try Data(contentsOf: dir.appendingPathComponent(name))
    }

    // MARK: Mac files load and are honoured

    @Test func macLedgerDedupStateAndAccountsLoadAndAreHonoured() async throws {
        let dir = try await dataDirectory(copying: ["takeover-ledger.json", "dedup-state.json"])
        let accountsBefore = try bytes(dir, "accounts.json")
        // The ledger's keys are "s/a/<start>" (fired) and "s/b/<start>" (snoozed until 07:55); the dedup lesson
        // says "sample a" on s/x and "sample b" on s/y are the same meeting.
        let h = try await EngineHarness(
            events: [
                Self.meeting("a", title: "Sample meeting A"),
                Self.meeting("b", title: "Sample meeting B"),
                Self.meeting("x1", title: "Sample A", calendar: "x", startsIn: 3600),
                Self.meeting("y1", title: "Sample B", calendar: "y", startsIn: 3600),
            ],
            directory: dir, sourceIDs: ["c1": "s"], clockStart: Self.meetingStart.addingTimeInterval(-600))
        await h.engine.send(.start)
        var state = await h.waitForState { $0.statuses["s"] == .ok }

        // Accounts load.
        #expect(state.accounts.map(\.connectionID) == ["c1"])
        #expect(state.accountStatus.errorMessage == nil)
        #expect(h.diagnostics.get().contains { $0.category == "ledger" && $0.message == "ledger loaded with 4 keys" })

        // The dedup lesson merges the two look-alikes (without it they are only manual candidates).
        let merged = try #require(state.events.first { $0.participants.count == 2 })
        #expect(Set(merged.participants.map(\.title)) == ["Sample A", "Sample B"])
        #expect(state.events.count == 3)

        // The fired meeting never takes over again; the snoozed one is armed for its snooze end and fires then.
        let a = try #require(state.events.first { $0.sourceEventID == "a" })
        #expect(state.ledger.hasFired(a))
        #expect(state.nextTakeover?.event.sourceEventID == "b")
        #expect(state.nextTakeover?.fireAt == Self.meetingStart.addingTimeInterval(-300))
        await h.clock.advance(by: 300)
        await h.waitUntil("the snoozed takeover") { h.takeoverDueIDs == ["b"] }
        let b = try #require(state.events.first { $0.sourceEventID == "b" })
        await h.engine.send(.dismiss(eventID: b.id))
        await h.clock.advance(by: 600)
        await h.settle()
        #expect(h.takeoverDueIDs == ["b"])
        state = await h.engine.currentState()
        #expect(state.ledger.hasFired(a) && state.ledger.hasFired(b))

        // accounts.json is only read: byte-identical after start, takeovers and stop.
        await h.engine.send(.stop)
        #expect(try bytes(dir, "accounts.json") == accountsBefore)
    }

    @Test func withoutTheMacDedupStateTheLookAlikesStaySeparate() async throws {
        let dir = try await dataDirectory(copying: [])
        let h = try await EngineHarness(
            events: [
                Self.meeting("x1", title: "Sample A", calendar: "x", startsIn: 3600),
                Self.meeting("y1", title: "Sample B", calendar: "y", startsIn: 3600),
            ],
            directory: dir, sourceIDs: ["c1": "s"], clockStart: Self.meetingStart.addingTimeInterval(-600))
        await h.engine.send(.start)
        let state = await h.waitForState { $0.statuses["s"] == .ok }
        #expect(state.events.count == 2)
    }

    @Test func legacySettingsDecodeAndAreNeverWrittenBackOnStart() async throws {
        let dir = try await dataDirectory(copying: ["settings-legacy.json"])
        let settingsBefore = try bytes(dir, "settings-legacy.json")
        let accountsBefore = try bytes(dir, "accounts.json")
        let storage = LegacyFixtureSettingsStorage(url: dir.appendingPathComponent("settings-legacy.json"))
        let h = try await EngineHarness(
            events: [.sample(id: "e1", startsIn: 600)], directory: dir, hostSettings: storage)
        await h.engine.send(.start)
        let state = await h.waitForState { $0.statuses["fake-c1"] == .ok }

        #expect(state.settings.takeover.enabled == false)
        #expect(state.settings.takeover.requireOtherAttendees == true)
        #expect(state.settings.takeover.leadTime == 120)
        #expect(state.nextTakeover == nil)
        await h.engine.send(.stop)
        #expect(storage.saves.get() == 0)
        #expect(try bytes(dir, "settings-legacy.json") == settingsBefore)
        #expect(try bytes(dir, "accounts.json") == accountsBefore)
    }

    // MARK: Review Focus 2: corrupt or unreadable files

    @Test func corruptLedgerAndDedupStateLoadEmptyWithDiagnosticsAndTheEngineStillStarts() async throws {
        let dir = try await dataDirectory(copying: [])
        try Data("{not json".utf8).write(to: dir.appendingPathComponent("takeover-ledger.json"))
        try Data("{\"lessons\": 7".utf8).write(to: dir.appendingPathComponent("dedup-state.json"))
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)], directory: dir)
        await h.engine.send(.start)
        let state = await h.waitForState { $0.statuses["fake-c1"] == .ok }

        let diagnostics = h.diagnostics.get()
        #expect(diagnostics.contains { $0.level == .warning && $0.category == "ledger" })
        #expect(diagnostics.contains { $0.level == .warning && $0.category == "dedup" })
        #expect(diagnostics.contains { $0.category == "ledger" && $0.message == "ledger loaded with 0 keys" })
        #expect(state.events.map(\.sourceEventID) == ["e1"])
        #expect(state.ledger.keyCount == 0)

        // The empty ledger is used: the meeting takes over at its lead time and is then recorded on disk.
        await h.clock.advance(by: 540)
        await h.waitUntil("the takeover") { h.takeoverDueIDs == ["e1"] }
        let e1 = try #require(state.events.first)
        #expect(LedgerStore(directory: dir, diagnostics: { _ in }).load().hasFired(e1))
    }

    @Test func anUnreadableAccountsFileIsNeverOverwritten() async throws {
        let dir = try TempDir.make()
        let accounts = dir.appendingPathComponent("accounts.json")
        let garbage = Data("not json".utf8)
        try garbage.write(to: accounts)
        let h = try await EngineHarness(events: [.sample(id: "e1", startsIn: 600)], directory: dir)
        await h.engine.send(.start)
        var state = await h.engine.currentState()

        #expect(state.accounts.isEmpty)
        #expect(state.events.isEmpty)
        #expect(state.accountStatus.errorMessage
            == "Your saved accounts could not be read, so none were loaded. Nothing was deleted.")
        #expect(try Data(contentsOf: accounts) == garbage)

        // Adding, removing and refreshing never write over the unreadable file.
        await h.engine.send(.addAccount(kindID: "fake"))
        await h.engine.send(.removeAccount(connectionID: "c1"))
        await h.engine.send(.refresh)
        await h.engine.send(.stop)
        state = await h.engine.currentState()
        #expect(state.accounts.isEmpty)
        #expect(state.accountStatus.errorMessage != nil)
        #expect(try Data(contentsOf: accounts) == garbage)
    }
}
