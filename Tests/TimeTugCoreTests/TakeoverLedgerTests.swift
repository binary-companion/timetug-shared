import Foundation
import Testing
@testable import TimeTugCore

private func roundTrip(_ ledger: TakeoverLedger) throws -> TakeoverLedger {
    try JSONDecoder().decode(TakeoverLedger.self, from: JSONEncoder().encode(ledger))
}

@Test func contentKeyIgnoresSourceIdentityAndCase() {
    let a = makeEvent("a", title: "Standup")
    let b = makeEvent("b", title: "STANDUP")
    #expect(a.id != b.id)
    #expect(a.contentKey == b.contentKey)
    #expect(a.contentKey != makeEvent("a", start: "2026-09-18T11:00:00Z").contentKey)
}

@Test func ledgerCodableRoundTripPreservesFiredAndSnoozed() throws {
    let fired = makeEvent("f", title: "Fired")
    let snoozed = makeEvent("s", title: "Snoozed", minutes: 60)
    var ledger = TakeoverLedger()
    ledger.markFired(fired, now: recordedAt)
    ledger.snooze(snoozed, for: 300, now: date("2026-09-18T09:59:00Z"))
    let loaded = try roundTrip(ledger)
    #expect(loaded == ledger)
    #expect(loaded.hasFired(fired))
    #expect(!loaded.hasFired(snoozed))
    #expect(loaded.entry(for: snoozed) == .snoozed(until: date("2026-09-18T10:04:00Z")))
}

@Test func ledgerPruningSurvivesRoundTrip() throws {
    let done = makeEvent("done", title: "Done", start: "2026-09-18T08:00:00Z")
    let running = makeEvent("running", title: "Running")
    var ledger = TakeoverLedger()
    ledger.markFired(done, now: recordedAt)
    ledger.markFired(running, now: recordedAt)
    var loaded = try roundTrip(ledger)
    loaded.prune(now: date("2026-09-18T10:10:00Z"))
    #expect(loaded.entry(for: done) == nil)
    #expect(loaded.entry(for: running) == .fired)
}

@Test func pruneRemovesBothKeys() {
    let event = makeEvent("a")
    var ledger = TakeoverLedger()
    ledger.markFired(event, now: recordedAt)
    ledger.prune(now: date("2026-09-18T10:31:00Z"))
    #expect(ledger.entry(for: makeEvent("other")) == nil) // same content, other id: content key gone too
    #expect(ledger == TakeoverLedger())
}

@Test func differentSourceIdWithSameContentCountsAsFired() {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent("old-id"), now: recordedAt)
    let resynced = makeEvent("new-id")
    #expect(ledger.hasFired(resynced))
    #expect(Scheduler.next(events: [resynced], settings: optedIn(), ledger: ledger,
                           now: date("2026-09-18T09:59:30Z")) == nil)
}

@Test func differentContentAndIdIsNotTheSameMeeting() {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent("a"), now: recordedAt)
    #expect(ledger.hasFired(makeEvent("a", title: "Renamed"))) // same id: an edit, not a new meeting
    #expect(!ledger.hasFired(makeEvent("b", title: "Renamed")))
    #expect(!ledger.hasFired(makeEvent("a", start: "2026-09-18T11:00:00Z")))
}

@Test func hasFiredIsFalseForSnoozedAndUnknown() {
    let event = makeEvent()
    var ledger = TakeoverLedger()
    #expect(!ledger.hasFired(event))
    ledger.snooze(event, for: 60, now: date("2026-09-18T10:00:00Z"))
    #expect(!ledger.hasFired(event))
    #expect(Scheduler.next(events: [event], settings: optedIn(), ledger: ledger,
                           now: date("2026-09-18T10:00:00Z")) != nil)
}

@Test func snoozeIsFoundThroughContentKey() {
    var ledger = TakeoverLedger()
    ledger.snooze(makeEvent("old", minutes: 60), for: 300, now: date("2026-09-18T09:59:00Z"))
    let next = Scheduler.next(events: [makeEvent("new", minutes: 60)], settings: optedIn(),
                              ledger: ledger, now: date("2026-09-18T09:59:00Z"))
    #expect(next?.fireAt == date("2026-09-18T10:04:00Z"))
}

@Test func acknowledgeInProgressMarksOnlyUnledgeredRunningEvents() {
    let now = date("2026-09-18T10:10:00Z")
    let running = makeEvent("running", title: "Running", start: "2026-09-18T10:00:00Z")
    let justStarted = makeEvent("recent", title: "Recent", start: "2026-09-18T10:09:00Z")
    let future = makeEvent("future", title: "Future", start: "2026-09-18T11:00:00Z")
    let ended = makeEvent("ended", title: "Ended", start: "2026-09-18T09:00:00Z")
    let snoozed = makeEvent("snoozed", title: "Snoozed", start: "2026-09-18T09:55:00Z", minutes: 60)
    var ledger = TakeoverLedger()
    ledger.snooze(snoozed, for: 300, now: date("2026-09-18T10:08:00Z"))
    let count = ledger.acknowledgeInProgress(
        events: [running, justStarted, future, ended, snoozed], now: now, grace: 120)
    #expect(count == 1)
    #expect(ledger.hasFired(running))
    #expect(ledger.entry(for: justStarted) == nil)
    #expect(ledger.entry(for: future) == nil)
    #expect(ledger.entry(for: ended) == nil)
    #expect(!ledger.hasFired(snoozed))
}

@Test func isSnoozedAndKeyCountReflectLedgerState() {
    let event = makeEvent()
    var ledger = TakeoverLedger()
    #expect(ledger.keyCount == 0)
    #expect(!ledger.isSnoozed(event))
    ledger.snooze(event, for: 60, now: date("2026-09-18T10:00:00Z"))
    #expect(ledger.isSnoozed(event))
    #expect(ledger.keyCount == 2)
    ledger.markFired(event, now: recordedAt)
    #expect(!ledger.isSnoozed(event))
}

// MARK: Retention

@Test func pruneReportsWhetherAnythingChanged() {
    let event = makeEvent()
    var ledger = TakeoverLedger()
    ledger.markFired(event, now: recordedAt)
    let changed1 = ledger.prune(now: date("2026-09-18T10:10:00Z"))
    #expect(!changed1)
    let changed2 = ledger.prune(now: date("2026-09-18T10:31:00Z"))
    #expect(changed2)
    let changed3 = ledger.prune(now: date("2026-09-18T10:32:00Z"))
    #expect(!changed3)
}

@Test func farFutureEventIsKeptUntilRetentionElapses() {
    // A bogus year-4001 end would never be pruned by the end rule.
    var event = makeEvent()
    event.end = Date(timeIntervalSince1970: 64_000_000_000)
    var ledger = TakeoverLedger()
    ledger.markFired(event, now: recordedAt)
    let changed4 = ledger.prune(now: recordedAt.addingTimeInterval(TakeoverLedger.retention - 1))
    #expect(!changed4)
    #expect(ledger.hasFired(event))
    let changed5 = ledger.prune(now: recordedAt.addingTimeInterval(TakeoverLedger.retention + 1))
    #expect(changed5)
    #expect(ledger == TakeoverLedger())
}

@Test func retentionIsSevenDays() {
    #expect(TakeoverLedger.retention == 7 * 24 * 60 * 60)
    #expect(TakeoverLedger.maxEntries == 2000)
}

@Test func sizeCapDropsOldestByRecordedAt() {
    var ledger = TakeoverLedger()
    let total = TakeoverLedger.maxEntries + 3
    for i in 0..<total {
        var event = makeEvent("e\(i)", title: "T\(i)")
        event.end = Date(timeIntervalSince1970: 64_000_000_000)
        ledger.markFired(event, now: recordedAt.addingTimeInterval(TimeInterval(i)))
    }
    let changed6 = ledger.prune(now: recordedAt.addingTimeInterval(TimeInterval(total)))
    #expect(changed6)
    #expect(ledger.keyCount == TakeoverLedger.maxEntries * 2)
    var oldest = makeEvent("e0", title: "T0")
    oldest.end = Date(timeIntervalSince1970: 64_000_000_000)
    var newest = makeEvent("e\(total - 1)", title: "T\(total - 1)")
    newest.end = oldest.end
    #expect(!ledger.hasFired(oldest))
    #expect(ledger.hasFired(newest))
}

@Test func decodingEntryWithoutRecordedAtDoesNotCrash() throws {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent(), now: recordedAt)
    var json = try #require(String(data: JSONEncoder().encode(ledger), encoding: .utf8))
    // Strip every recordedAt member, simulating a file from before the field existed.
    json = json.replacingOccurrences(of: "\"recordedAt\"", with: "\"ignored\"")
    let loaded = try JSONDecoder().decode(TakeoverLedger.self, from: Data(json.utf8))
    #expect(!loaded.hasFired(makeEvent())) // dropped rather than trusted
}

private func merged(_ primary: CalendarEvent, with others: [CalendarEvent]) -> CalendarEvent {
    var event = primary
    event.mergedMembers = ([primary] + others).map {
        MergedMember($0)
    }
    return event
}

@Test func mergedEventCountsAsFiredWhenAnyMemberFired() {
    let original = makeEvent("1", title: "Scott: Doctor")
    let official = makeEvent("2", title: "Intermountain Health")
    var ledger = TakeoverLedger()
    ledger.markFired(original, now: recordedAt)
    #expect(ledger.hasFired(merged(official, with: [original])))
}

@Test func firingMergedEventMarksEveryMember() {
    let original = makeEvent("1", title: "Scott: Doctor")
    let official = makeEvent("2", title: "Intermountain Health")
    var ledger = TakeoverLedger()
    ledger.markFired(merged(official, with: [original]), now: recordedAt)
    #expect(ledger.hasFired(original))
    #expect(ledger.hasFired(official))
}
