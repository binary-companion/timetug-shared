import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T09:59:00Z")

private func evaluate(
    _ event: CalendarEvent, current: [CalendarEvent]? = nil, settings: TakeoverSettings = optedIn(),
    ledger: TakeoverLedger = TakeoverLedger(), now: Date = now, overlayVisible: Bool = false
) -> TakeoverGuard.Decision {
    TakeoverGuard.evaluate(event: event, currentEvents: current ?? [event], settings: settings,
                           ledger: ledger, now: now, overlayVisible: overlayVisible)
}

@Test func guardPresentsNormalEvent() {
    #expect(evaluate(makeEvent()) == .present)
}

@Test func guardSuppressesWhenOverlayVisibleFirst() {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent(), now: recordedAt)
    #expect(evaluate(makeEvent(), current: [], ledger: ledger, overlayVisible: true) == .suppress(.overlayVisible))
}

@Test func guardSuppressesEventMissingFromSnapshot() {
    #expect(evaluate(makeEvent(), current: [makeEvent("other", title: "Other")]) == .suppress(.notInSnapshot))
    #expect(evaluate(makeEvent(), current: []) == .suppress(.notInSnapshot))
}

@Test func guardMatchesSnapshotByContentKey() {
    #expect(evaluate(makeEvent("old"), current: [makeEvent("new")]) == .present)
}

@Test func guardSuppressesEndedBeforeQualification() {
    let event = makeEvent()
    let late = date("2026-09-18T10:30:00Z")
    var declined = event
    declined.responseStatus = .declined
    #expect(evaluate(event, now: late) == .suppress(.ended))
    #expect(evaluate(event, current: [declined], now: late) == .suppress(.ended))
}

@Test func guardUsesEditedCurrentCopy() {
    let event = makeEvent()
    var declined = event
    declined.responseStatus = .declined
    #expect(evaluate(event, current: [declined]) == .suppress(.noLongerQualifies))
    var optedOut = event
    optedOut.calendarID = "family"
    #expect(evaluate(event, current: [optedOut]) == .suppress(.noLongerQualifies))
}

@Test func guardSuppressesAlreadyFiredAndChecksBeforeQualifying() {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent(), now: recordedAt)
    #expect(evaluate(makeEvent(), ledger: ledger) == .suppress(.alreadyFired))
    var declined = makeEvent()
    declined.responseStatus = .declined
    #expect(evaluate(makeEvent(), current: [declined], ledger: ledger) == .suppress(.noLongerQualifies))
}

@Test func guardSuppressesFiredThroughContentKey() {
    var ledger = TakeoverLedger()
    ledger.markFired(makeEvent("old"), now: recordedAt)
    #expect(evaluate(makeEvent("new"), ledger: ledger) == .suppress(.alreadyFired))
}

@Test func guardPresentsSnoozedEvent() {
    var ledger = TakeoverLedger()
    ledger.snooze(makeEvent(), for: 60, now: now)
    #expect(evaluate(makeEvent(), ledger: ledger) == .present)
}
