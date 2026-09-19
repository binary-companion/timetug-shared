import Foundation
import Testing
@testable import TimeTugCore

private let t0 = date("2026-09-18T09:00:00Z")
private let engine = EngineInfo(id: "fake-ai", displayName: "Fake AI", isOnDevice: true)

@Test func adjudicationEventTruncatesNotesAndDropsEmailsByConstruction() {
    let event = makeEvent("1", title: "Doctor", notes: String(repeating: "x", count: 900),
                          attendees: [Attendee(name: "Kristin", email: "k@x.com"), Attendee(name: nil, email: "n@x.com")])
    let info = CalendarInfo(sourceID: "fake", calendarID: "cal", title: "Personal", accountName: "iCloud")
    let projected = AdjudicationEvent(event, calendar: info)
    #expect(projected.notes?.count == AdjudicationEvent.maxNotesLength)
    #expect(projected.attendeeNames == ["Kristin"])
    #expect(projected.calendarTitle == "Personal")
    #expect(projected.accountName == "iCloud")
}

@Test func adjudicationEventDropsAttendeeNamesThatAreEmailAddresses() {
    let event = makeEvent("1", title: "Doctor", attendees: [
        Attendee(name: "kristin@example.com", email: "kristin@example.com"),
        Attendee(name: "Scott <scott@example.com>", email: nil),
        Attendee(name: "", email: "e@x.com"),
        Attendee(name: "Kristin", email: "k@x.com")])
    #expect(AdjudicationEvent(event, calendar: nil).attendeeNames == ["Kristin"])
}

@Test func verdictCacheKeepsEntriesByAgeNotByEventEnd() {
    var cache = VerdictCache()
    cache.store(AdjudicationVerdict(requestID: "a", answer: .same), engine: engine, end: t0.addingTimeInterval(3600), now: t0)
    cache.store(AdjudicationVerdict(requestID: "b", answer: .unsure), engine: engine, end: t0.addingTimeInterval(60), now: t0)
    #expect(cache.entry(for: "a")?.answer == .same)
    #expect(cache.entry(for: "a")?.engine == engine)
    let droppedFresh = cache.prune(now: t0.addingTimeInterval(120))    // "b" ended but is still fresh
    #expect(!droppedFresh)
    #expect(cache.entry(for: "b") != nil)
    let droppedAtLimit = cache.prune(now: t0.addingTimeInterval(VerdictCache.retention))
    #expect(!droppedAtLimit)
    let droppedOld = cache.prune(now: t0.addingTimeInterval(VerdictCache.retention + 1))
    #expect(droppedOld)
    #expect(cache.entry(for: "a") == nil)
    #expect(cache.entry(for: "b") == nil)
}

@Test func verdictCacheIsCappedOldestFirst() {
    var cache = VerdictCache()
    for i in 0..<(VerdictCache.maxEntries + 3) {
        cache.store(AdjudicationVerdict(requestID: "r\(i)", answer: .same), engine: engine,
                    end: t0.addingTimeInterval(86_400), now: t0.addingTimeInterval(TimeInterval(i)))
    }
    cache.prune(now: t0)
    #expect(cache.entries.count == VerdictCache.maxEntries)
    #expect(cache.entry(for: "r0") == nil)
}

@Test func verdictCacheCodableRoundTrip() throws {
    var cache = VerdictCache()
    cache.store(AdjudicationVerdict(requestID: "a", answer: .different), engine: engine, end: t0.addingTimeInterval(60), now: t0)
    #expect(try JSONDecoder().decode(VerdictCache.self, from: JSONEncoder().encode(cache)) == cache)
}

@Test func fingerprintIsStableAndSensitive() {
    #expect(Fingerprint.fnv1a("abc") == Fingerprint.fnv1a("abc"))
    #expect(Fingerprint.fnv1a("abc") != Fingerprint.fnv1a("abd"))
}
