import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T10:00:00Z")

private actor NoticeSource: TimeTugCore.CalendarSource {
    nonisolated let id: String
    nonisolated let displayName = "Notice"
    private var noticeList: [SourceNotice]
    private var fails = false

    init(id: String, notices: [SourceNotice]) { self.id = id; noticeList = notices }

    func set(notices: [SourceNotice]) { noticeList = notices }
    func setFailing(_ value: Bool) { fails = value }

    func calendars() async throws -> [CalendarInfo] { [] }
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] {
        if fails { throw TimeTugCore.SourceError.authExpired }
        return []
    }
    func notices() async -> [SourceNotice] { noticeList }
    nonisolated func changes() -> AsyncStream<Void> { AsyncStream { $0.finish() } }
}

private let warning = SourceNotice(kind: .unreadableRecurrence, count: 2)

@Test func aSourcesNoticesReachTheSnapshotAndAreClearedWhenItIsFixed() async {
    let source = NoticeSource(id: "a", notices: [warning])
    let plain = FakeSource(id: "b")
    let store = CalendarStore(sources: [source, plain], calendar: utcCalendar)
    var snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.notices == ["a": [warning]])
    await source.set(notices: [])
    snapshot = await store.refresh(now: now, leadTime: 60)
    #expect(snapshot.notices.isEmpty)
}

@Test func noticesAreClearedWhenTheSourceFailsOrIsRemoved() async {
    let source = NoticeSource(id: "a", notices: [warning])
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    _ = await store.refresh(now: now, leadTime: 60)
    await source.setFailing(true)
    #expect(await store.refresh(now: now, leadTime: 60).notices.isEmpty)
    await source.setFailing(false)
    #expect(await store.refresh(now: now, leadTime: 60).notices.count == 1)
    await store.setSources([])
    #expect(await store.setInferenceEnabled(false, now: now).notices.isEmpty)
}

@Test func aSourceWithoutNoticesGivesNone() async {
    let store = CalendarStore(sources: [FakeSource(id: "a")], calendar: utcCalendar)
    #expect(await store.refresh(now: now, leadTime: 60).notices.isEmpty)
}
