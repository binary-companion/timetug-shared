import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private let t0 = date("2026-09-18T09:00:00Z")
private let engine = EngineInfo(id: "fake-ai", displayName: "Fake AI", isOnDevice: true)

@Test func adjudicationEventTruncatesNotesAndDropsEmailsByConstruction() {
    let event = makeEvent("1", title: "Doctor", notes: String(repeating: "x", count: 900),
                          attendees: [CalendarCore.Attendee(name: "Kristin", email: "k@x.com"), CalendarCore.Attendee(name: nil, email: "n@x.com")])
    let info = CalendarInfo(sourceID: "fake", calendarID: "cal", title: "Personal", accountName: "iCloud")
    let projected = AdjudicationEvent(event, calendar: info)
    #expect(projected.notes?.count == AdjudicationEvent.maxNotesLength)
    #expect(projected.attendeeNames == ["Kristin"])
    #expect(projected.calendarTitle == "Personal")
}

@Test func adjudicationEventDropsAttendeeNamesThatAreEmailAddresses() {
    let event = makeEvent("1", title: "Doctor", attendees: [
        CalendarCore.Attendee(name: "kristin@example.com", email: "kristin@example.com"),
        CalendarCore.Attendee(name: "Scott <scott@example.com>", email: nil),
        CalendarCore.Attendee(name: "", email: "e@x.com"),
        CalendarCore.Attendee(name: "Kristin", email: "k@x.com")])
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

@Test func fingerprintMatchesKnownFNV1aVectors() {
    #expect(Fingerprint.fnv1a("") == "cbf29ce484222325")
    #expect(Fingerprint.fnv1a("a") == "af63dc4c8601ec8c")
}

@Test func canonicalInputBoundsUnicodeWithoutSplittingCharacters() {
    let family = "👨‍👩‍👧‍👦"
    let event = makeEvent("1", title: String(repeating: family, count: 300),
                          location: String(repeating: family, count: 600),
                          notes: String(repeating: family, count: 600),
                          attendees: (0..<20).map { _ in CalendarCore.Attendee(name: String(repeating: family, count: 90), email: nil) })
    let info = CalendarInfo(sourceID: "fake", calendarID: "cal", title: String(repeating: family, count: 200))
    let projected = AdjudicationEvent(event, calendar: info)
    #expect(projected.title.count == 256)
    #expect(projected.location?.count == 512)
    #expect(projected.notes?.count == 500)
    #expect(projected.calendarTitle?.count == 128)
    #expect(projected.attendeeNames.count == 10)
    #expect(projected.attendeeNames.allSatisfy { $0.count == 80 && !$0.contains("�") })
}

@Test func canonicalCacheIdentityIncludesPresentedDataAndPolicy() {
    let a = AdjudicationEvent(makeEvent("1", title: "Doctor"), calendar: nil)
    var b = AdjudicationEvent(makeEvent("2", title: "Clinic"), calendar: nil)
    let base = AdjudicationRequest(id: "", first: a, second: b, lessons: [])
    let key = base.input.cacheKey(engineID: "apple", policyVersion: 2)
    #expect(base.input.cacheKey(engineID: "apple", policyVersion: 2) == key)
    #expect(base.input.cacheKey(engineID: "other", policyVersion: 2) != key)
    #expect(base.input.cacheKey(engineID: "apple", policyVersion: 3) != key)
    b.calendarTitle = "Kristin"
    #expect(AdjudicationRequest(id: "", first: a, second: b, lessons: []).input.cacheKey(engineID: "apple", policyVersion: 2) != key)
    b.calendarTitle = nil
    b.attendeeNames = ["Kristin"]
    #expect(AdjudicationRequest(id: "", first: a, second: b, lessons: []).input.cacheKey(engineID: "apple", policyVersion: 2) != key)
}

@Test func cacheKeyIgnoresTextBeyondRenderedPromptBudget() {
    var a = AdjudicationEvent(makeEvent("1", title: String(repeating: "\\\n", count: 256)), calendar: nil)
    let b = AdjudicationEvent(makeEvent("2", title: "Clinic"), calendar: nil)
    let first = AdjudicationRequest(id: "", first: a, second: b, lessons: []).input.cacheKey(engineID: "apple")
    a.title.replaceSubrange(a.title.index(before: a.title.endIndex)..., with: "Z")
    let second = AdjudicationRequest(id: "", first: a, second: b, lessons: []).input.cacheKey(engineID: "apple")
    #expect(first == second)
}

@Test func equivalentSwappedPairAndLessonUsageKeepCacheIdentity() {
    let firstEvent = makeEvent("1", title: "Doctor")
    let secondEvent = makeEvent("2", title: "Clinic")
    let a = AdjudicationEvent(firstEvent, calendar: nil)
    let b = AdjudicationEvent(secondEvent, calendar: nil)
    var book = LessonBook()
    book.record(MergedMember(firstEvent), MergedMember(secondEvent), decision: .same, now: t0)
    let forward = AdjudicationRequest(id: "", first: a, second: b, lessons: book.lessons,
                                      startOffsetMinutes: 10, endOffsetMinutes: -5,
                                      firstDetails: "location", secondDetails: "bare")
    var touched = book.lessons
    touched[0].lastUsed = t0.addingTimeInterval(100)
    let reverse = AdjudicationRequest(id: "", first: b, second: a, lessons: touched,
                                      startOffsetMinutes: -10, endOffsetMinutes: 5,
                                      firstDetails: "bare", secondDetails: "location")
    #expect(forward.input.cacheKey(engineID: "apple") == reverse.input.cacheKey(engineID: "apple"))
    touched[0].decision = .different
    #expect(forward.input.cacheKey(engineID: "apple") !=
            AdjudicationRequest(id: "", first: b, second: a, lessons: touched,
                                startOffsetMinutes: -10, endOffsetMinutes: 5,
                                firstDetails: "bare", secondDetails: "location").input.cacheKey(engineID: "apple"))
}
