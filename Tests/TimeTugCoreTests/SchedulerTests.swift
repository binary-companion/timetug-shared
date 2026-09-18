import Foundation
import Testing
@testable import TimeTugCore

private let tenAM = date("2026-09-18T10:00:00Z")

@Test func firesAtStartMinusLeadTime() {
    let event = makeEvent(start: "2026-09-18T10:00:00Z")
    let next = Scheduler.next(events: [event], settings: optedIn { $0.leadTime = 120 },
                              ledger: TakeoverLedger(), now: date("2026-09-18T09:00:00Z"))
    #expect(next?.fireAt == date("2026-09-18T09:58:00Z"))
    #expect(next?.event == event)
}

@Test func zeroLeadTimeFiresAtStart() {
    let next = Scheduler.next(events: [makeEvent()], settings: optedIn { $0.leadTime = 0 },
                              ledger: TakeoverLedger(), now: date("2026-09-18T09:00:00Z"))
    #expect(next?.fireAt == tenAM)
}

@Test func lateFireIsImmediateWhileMeetingInProgress() {
    let now = date("2026-09-18T10:05:00Z")
    let next = Scheduler.next(events: [makeEvent()], settings: optedIn(),
                              ledger: TakeoverLedger(), now: now)
    #expect(next?.fireAt == now)
}

@Test func endedMeetingsAreIgnored() {
    let next = Scheduler.next(events: [makeEvent()], settings: optedIn(),
                              ledger: TakeoverLedger(), now: date("2026-09-18T10:30:00Z"))
    #expect(next == nil)
}

@Test func firedEventsDoNotRepeat() {
    let event = makeEvent()
    var ledger = TakeoverLedger()
    ledger.markFired(event, now: recordedAt)
    #expect(Scheduler.next(events: [event], settings: optedIn(), ledger: ledger,
                           now: date("2026-09-18T09:59:30Z")) == nil)
}

@Test func rescheduledEventCountsAsNew() {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent(start: "2026-09-18T10:00:00Z"), now: recordedAt)
    let moved = makeEvent(start: "2026-09-18T11:00:00Z")
    #expect(Scheduler.next(events: [moved], settings: optedIn(), ledger: ledger,
                           now: date("2026-09-18T09:00:00Z")) != nil)
}

@Test func snoozeRefiresAtSnoozeEnd() {
    let event = makeEvent(minutes: 60)
    var ledger = TakeoverLedger()
    let now = date("2026-09-18T09:59:00Z")
    ledger.markFired(event, now: recordedAt)
    ledger.snooze(event, for: 300, now: now)
    let next = Scheduler.next(events: [event], settings: optedIn(), ledger: ledger, now: now)
    #expect(next?.fireAt == date("2026-09-18T10:04:00Z"))
}

@Test func snoozeIsCappedAtMeetingEnd() {
    let event = makeEvent(minutes: 5)
    var ledger = TakeoverLedger()
    let now = date("2026-09-18T10:03:00Z")
    ledger.snooze(event, for: 600, now: now)
    // Capped at 10:05 == end, so nothing left to fire.
    #expect(Scheduler.next(events: [event], settings: optedIn(), ledger: ledger, now: now) == nil)
}

@Test func picksEarliestQualifyingEvent() {
    let later = makeEvent("late", start: "2026-09-18T11:00:00Z")
    let sooner = makeEvent("soon", start: "2026-09-18T10:00:00Z")
    let next = Scheduler.next(events: [later, sooner], settings: optedIn(),
                              ledger: TakeoverLedger(), now: date("2026-09-18T09:00:00Z"))
    #expect(next?.event == sooner)
}

@Test func nonQualifyingEventsAreSkipped() {
    let next = Scheduler.next(events: [makeEvent(calendarID: "family")], settings: optedIn(),
                              ledger: TakeoverLedger(), now: date("2026-09-18T09:00:00Z"))
    #expect(next == nil)
}

@Test func afterMidnightMeetingFiresBeforeMidnight() {
    let event = makeEvent(start: "2026-09-19T00:05:00Z")
    let next = Scheduler.next(events: [event], settings: optedIn { $0.leadTime = 600 },
                              ledger: TakeoverLedger(), now: date("2026-09-18T23:00:00Z"))
    #expect(next?.fireAt == date("2026-09-18T23:55:00Z"))
}

@Test func ledgerPrunesByEventEnd() {
    let done = makeEvent("done", start: "2026-09-18T08:00:00Z")
    let running = makeEvent("running", start: "2026-09-18T10:00:00Z")
    var ledger = TakeoverLedger()
    ledger.markFired(done, now: recordedAt)
    ledger.markFired(running, now: recordedAt)
    ledger.prune(now: date("2026-09-18T10:10:00Z"))
    #expect(ledger.entry(for: done) == nil)
    #expect(ledger.entry(for: running) != nil)
}

@Test func requestMarksStartedAndFiltersSnoozeOptions() {
    let event = makeEvent(minutes: 30, conferenceURL: URL(string: "https://meet.google.com/a-b-c"))
    let before = TakeoverRequest.make(for: event, now: date("2026-09-18T09:59:00Z"))
    #expect(!before.hasStarted)
    #expect(before.joinURL != nil)
    #expect(before.snoozeOptions == [60, 300, 600])

    let late = TakeoverRequest.make(for: event, now: date("2026-09-18T10:28:00Z"))
    #expect(late.hasStarted)
    #expect(late.snoozeOptions == [60])
}
