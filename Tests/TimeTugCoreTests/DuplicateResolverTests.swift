import Foundation
import Testing
@testable import TimeTugCore

private let t0 = date("2026-09-18T09:00:00Z")
private let engine = EngineInfo(id: "test", displayName: "Test AI", isOnDevice: true)

private let doctor = makeEvent("1", title: "Scott: Doctor", minutes: 60, calendarID: "personal", others: 0)
private let official = makeEvent("2", title: "Intermountain Health", minutes: 60, calendarID: "work",
                                 location: "1234 Main St, Logan", notes: "Bring insurance card")
private let dance = makeEvent("3", title: "Kristin: Logan Dance", minutes: 60, calendarID: "personal", others: 0)

private func resolve(_ events: [CalendarEvent], lessons: LessonBook = LessonBook(),
                     verdicts: VerdictCache? = nil) -> DuplicateResolution {
    DuplicateResolver.resolve(events: events, calendars: [], lessons: lessons, verdicts: verdicts)
}

private func cache(answering answer: AdjudicationVerdict.Answer, for resolution: DuplicateResolution) -> VerdictCache {
    var cache = VerdictCache()
    for request in resolution.pending {
        cache.store(AdjudicationVerdict(requestID: request.id, answer: answer), engine: engine,
                    end: t0.addingTimeInterval(86_400), now: t0)
    }
    return cache
}

@Test func exactDuplicatesMergeWithRuleProvenanceAndKeepTheFirstOnATie() {
    let a = makeEvent("1", title: "Sync", calendarID: "family")
    let b = makeEvent("2", title: "sync", calendarID: "work")
    let result = resolve([a, b])
    #expect(result.events.count == 1)
    #expect(result.events[0].calendarKey == "fake/family")
    #expect(result.events[0].additionalCalendarKeys == ["fake/work"])
    #expect(result.events[0].mergeProvenance == .rule)
    #expect(result.events[0].mergedMembers.count == 2)
}

@Test func richerCopyBecomesPrimaryAndBorrowsMissingDetails() {
    let bare = makeEvent("1", title: "Sync", calendarID: "a")
    let rich = makeEvent("2", title: "Sync", calendarID: "b", notes: "Join https://acme.zoom.us/j/123")
    let merged = resolve([bare, rich]).events[0]
    #expect(merged.calendarKey == "fake/b")
    #expect(merged.additionalCalendarKeys == ["fake/a"])
}

@Test func ambiguousPairIsPendingWhenInferenceIsOnAndUnknown() {
    let result = resolve([doctor, official], verdicts: VerdictCache())
    #expect(result.events.count == 2)
    #expect(result.pending.count == 1)
    #expect(result.pending[0].first.title == "Intermountain Health")   // richer first
    #expect(result.pending[0].second.title == "Scott: Doctor")
}

@Test func rulesOnlyNeverProducesPending() {
    let result = resolve([doctor, official], verdicts: nil)
    #expect(result.events.count == 2)
    #expect(result.pending.isEmpty)
}

@Test func cachedSameVerdictMergesWithInferenceProvenance() {
    let verdicts = cache(answering: .same, for: resolve([doctor, official], verdicts: VerdictCache()))
    let result = resolve([doctor, official], verdicts: verdicts)
    #expect(result.events.count == 1)
    #expect(result.pending.isEmpty)
    let merged = result.events[0]
    #expect(merged.title == "Intermountain Health")
    #expect(merged.mergeProvenance == .inference(engineID: "test", engineName: "Test AI"))
    #expect(merged.additionalCalendarKeys == ["fake/personal"])
    #expect(merged.mergedMembers.map(\.title).sorted() == ["Intermountain Health", "Scott: Doctor"])
}

@Test func unsureAndDifferentVerdictsDoNotMerge() {
    for answer in [AdjudicationVerdict.Answer.unsure, .different] {
        let verdicts = cache(answering: answer, for: resolve([doctor, official], verdicts: VerdictCache()))
        let result = resolve([doctor, official], verdicts: verdicts)
        #expect(result.events.count == 2)
        #expect(result.pending.isEmpty)
    }
}

@Test func sameCalendarEventsAtTheSameTimeStaySeparate() {
    let result = resolve([doctor, dance], verdicts: VerdictCache())
    #expect(result.events.count == 2)
    #expect(result.pending.isEmpty)
}

@Test func lessonSameMergesEvenWithoutInferenceAndIsReported() {
    var lessons = LessonBook()
    lessons.record(MergedMember(doctor), MergedMember(official), decision: .same, now: t0)
    let result = resolve([doctor, official], lessons: lessons, verdicts: nil)
    #expect(result.events.count == 1)
    #expect(result.events[0].mergeProvenance == .userConfirmed)
    #expect(result.usedLessonKeys.count == 1)
}

@Test func lessonDifferentBeatsAStrongRuleAndOffersAManualMerge() {
    let zoom = "https://acme.zoom.us/j/9"
    let a = makeEvent("1", title: "Weekly", calendarID: "a", location: zoom)
    let b = makeEvent("2", title: "Team sync", calendarID: "b", location: zoom)
    #expect(resolve([a, b]).events.count == 1)
    var lessons = LessonBook()
    lessons.record(MergedMember(a), MergedMember(b), decision: .different, now: t0)
    let result = resolve([a, b], lessons: lessons)
    #expect(result.events.count == 2)
    #expect(result.candidates[a.id]?.map(\.id) == [b.id])
    #expect(result.candidates[b.id]?.map(\.id) == [a.id])
}

@Test func lessonSameStillRespectsTheTimeGate() {
    var lessons = LessonBook()
    let late = makeEvent("2", title: "Intermountain Health", start: "2026-09-18T15:00:00Z", minutes: 60, calendarID: "work")
    lessons.record(MergedMember(doctor), MergedMember(late), decision: .same, now: t0)
    #expect(resolve([doctor, late], lessons: lessons).events.count == 2)
}

@Test func mergeIsSkippedWhenAnyGroupMemberBlocksIt() {
    let zoom = "https://acme.zoom.us/j/9"
    let a = makeEvent("1", title: "One", calendarID: "a", location: zoom)
    let b = makeEvent("2", title: "Two", calendarID: "b", location: zoom)
    let c = makeEvent("3", title: "Three", calendarID: "a", location: zoom)   // same calendar as a
    let result = resolve([a, b, c])
    #expect(result.events.count == 2)
}

@Test func candidatesListSeparateLookAlikes() {
    let result = resolve([doctor, official], verdicts: nil)
    #expect(result.candidates[doctor.id]?.map(\.id) == [official.id])
    #expect(result.candidates[official.id]?.map(\.id) == [doctor.id])
}

@Test func equalStartAndTitleEventsSortDeterministicallyById() {
    let a = makeEvent("1", title: "Sync", calendarID: "a")
    let b = makeEvent("2", title: "Sync", calendarID: "b")
    var lessons = LessonBook()
    lessons.record(MergedMember(a), MergedMember(b), decision: .different, now: t0)
    let forward = resolve([a, b], lessons: lessons).events.map(\.id)
    let backward = resolve([b, a], lessons: lessons).events.map(\.id)
    #expect(forward.count == 2)
    #expect(forward == backward)
}

private let zoom = URL(string: "https://acme.zoom.us/j/123456")!

@Test func mergedEventKeepsTheLargestAttendeeCountSoTakeoverStillQualifies() {
    let work = makeEvent("1", title: "Planning", calendarID: "cal", others: 5, conferenceURL: zoom)
    let personal = makeEvent("2", title: "Planning copy", calendarID: "personal", others: 0,
                             location: "Room 4", notes: "Agenda", conferenceURL: zoom)
    #expect(TakeoverPolicy.qualifies(work, settings: optedIn()))
    let merged = resolve([work, personal]).events
    #expect(merged.count == 1)
    #expect(merged[0].calendarKey == "fake/personal")        // the richer copy is the primary
    #expect(merged[0].otherAttendeeCount == 5)
    #expect(TakeoverPolicy.qualifies(merged[0], settings: optedIn()))
}

@Test func mergedEventIsNotDeclinedWhenAnyCopyIsNotDeclined() {
    let work = makeEvent("1", title: "Planning", calendarID: "cal", others: 3, status: .accepted, conferenceURL: zoom)
    let personal = makeEvent("2", title: "Planning copy", calendarID: "personal", others: 3, status: .declined,
                             location: "Room 4", notes: "Agenda", conferenceURL: zoom)
    #expect(TakeoverPolicy.qualifies(work, settings: optedIn()))
    let merged = resolve([work, personal]).events
    #expect(merged.count == 1)
    #expect(merged[0].calendarKey == "fake/personal")
    #expect(merged[0].responseStatus == .accepted)
    #expect(TakeoverPolicy.qualifies(merged[0], settings: optedIn()))
}

@Test func mergedResponseStatusPrefersTheMostAttending() {
    func status(_ list: [ResponseStatus]) -> ResponseStatus {
        let events = list.enumerated().map { makeEvent("\($0.offset)", title: "Sync", calendarID: "c\($0.offset)", status: $0.element) }
        return resolve(events).events[0].responseStatus
    }
    #expect(status([.declined, .unknown]) == .unknown)
    #expect(status([.unknown, .pending]) == .pending)
    #expect(status([.pending, .tentative]) == .tentative)
    #expect(status([.tentative, .accepted, .declined]) == .accepted)
    #expect(status([.declined, .declined]) == .declined)
}

@Test func groupsAreCappedAtFourEventsAndTheRestStaySeparate() {
    let events = (0..<5).map { makeEvent("e\($0)", title: "Sync", calendarID: "c\($0)") }
    let result = resolve(events).events
    #expect(result.count == 2)
    let group = result.first { !$0.mergedMembers.isEmpty }!
    #expect(group.mergedMembers.map(\.calendarKey) == ["fake/c0", "fake/c1", "fake/c2", "fake/c3"])
    #expect(group.mergeProvenance == .rule)
    let alone = result.first { $0.mergedMembers.isEmpty }!
    #expect(alone.calendarKey == "fake/c4")
}

@Test func aUserConfirmedLinkMakesAThreeWayMergeUserConfirmed() {
    let a = makeEvent("1", title: "Sync", calendarID: "a")
    let b = makeEvent("2", title: "Sync", calendarID: "b")
    let c = makeEvent("3", title: "Sync", calendarID: "c")
    var lessons = LessonBook()
    lessons.record(MergedMember(a), MergedMember(b), decision: .same, now: t0)
    let result = resolve([a, b, c], lessons: lessons).events
    #expect(result.count == 1)
    #expect(result[0].mergedMembers.count == 3)
    #expect(result[0].mergeProvenance == .userConfirmed)
}

// MARK: conference link survives a merge

private func mergedEvent(_ events: [CalendarEvent]) -> CalendarEvent {
    let result = resolve(events).events
    #expect(result.count == 1)
    return result[0]
}

@Test func mergeKeepsALinkFoundOnlyInTheNonPrimaryCopysNotes() {
    let personal = makeEvent("1", title: "Review", calendarID: "personal", others: 1,
                             location: "Home office", notes: "prep slides", externalUID: "u1")
    let work = makeEvent("2", title: "Review", calendarID: "cal", others: 4,
                         notes: "Join https://teams.microsoft.com/l/meetup-join/abc", externalUID: "u1")
    let merged = mergedEvent([personal, work])
    #expect(merged.notes == "prep slides")   // personal is the primary
    #expect(merged.conferenceURL?.host == "teams.microsoft.com")
    #expect(TakeoverPolicy.qualifies(merged, settings: optedIn { $0.requireConferenceLink = true }))
}

@Test func aGenericURLOnThePrimaryNeverBeatsAProviderLinkOnAnotherCopy() {
    let primary = makeEvent("1", title: "Review", calendarID: "personal", others: 1,
                            location: "Home office", notes: "prep", url: URL(string: "https://example.com/agenda"), externalUID: "u1")
    let other = makeEvent("2", title: "Review", calendarID: "work", others: 1,
                          notes: "https://acme.zoom.us/j/123", externalUID: "u1")
    #expect(mergedEvent([primary, other]).conferenceURL?.host == "acme.zoom.us")
}

@Test func theStructuredLinkOnAnyCopyCountsAndThePrimaryWinsTies() {
    let a = makeEvent("1", title: "Review", calendarID: "personal", notes: "x", externalUID: "u1")
    let b = makeEvent("2", title: "Review", calendarID: "work", conferenceURL: URL(string: "https://chime.aws/222"), externalUID: "u1")
    #expect(mergedEvent([a, b]).conferenceURL?.absoluteString == "https://chime.aws/222")
    let c = makeEvent("3", title: "Review", calendarID: "x", notes: "https://acme.zoom.us/j/1 more", externalUID: "u1")
    let d = makeEvent("4", title: "Review", calendarID: "y", notes: "https://acme.zoom.us/j/2", externalUID: "u1")
    #expect(mergedEvent([c, d]).conferenceURL?.absoluteString == "https://acme.zoom.us/j/1")
}

@Test func unmergedEventsKeepTheirOwnConferenceURL() {
    let plain = makeEvent("1", title: "Solo", notes: "https://acme.zoom.us/j/9")
    #expect(resolve([plain]).events[0].conferenceURL == nil)
    let structured = makeEvent("2", title: "Other", start: "2026-09-18T15:00:00Z", conferenceURL: URL(string: "https://chime.aws/1"))
    #expect(resolve([structured]).events[0].conferenceURL?.absoluteString == "https://chime.aws/1")
}
