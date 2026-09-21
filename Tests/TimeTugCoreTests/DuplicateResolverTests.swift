import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private let t0 = date("2026-09-18T09:00:00Z")
private let engine = EngineInfo(id: "test", displayName: "Test AI", isOnDevice: true)

private let doctor = makeEvent("1", title: "Scott: Doctor", minutes: 60, calendarID: "personal", others: 0)
private let official = makeEvent("2", title: "Intermountain Health", minutes: 60, calendarID: "work",
                                 location: "1234 Main St, Logan", notes: "Bring insurance card")
private let dance = makeEvent("3", title: "Kristin: Logan Dance", minutes: 60, calendarID: "personal", others: 0)

private func resolve(_ events: [TimeTugCalendarEvent], lessons: LessonBook = LessonBook(),
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
    func status(_ list: [CalendarCore.ResponseStatus?]) -> CalendarCore.ResponseStatus? {
        let events = list.enumerated().map { makeEvent("\($0.offset)", title: "Sync", calendarID: "c\($0.offset)", status: $0.element) }
        return resolve(events).events[0].responseStatus
    }
    #expect(status([.declined, nil]) == nil)
    #expect(status([nil, .needsAction]) == .needsAction)
    #expect(status([.needsAction, .tentative]) == .tentative)
    #expect(status([.tentative, .accepted, .declined]) == .accepted)
    #expect(status([.declined, .declined]) == .declined)
}

@Test func fiveExactDuplicatesAcrossCalendarsMergeIntoOneCard() {
    let events = (0..<5).map { makeEvent("e\($0)", title: "Sync", calendarID: "c\($0)") }
    let result = resolve(events).events
    #expect(result.count == 1)
    #expect(result[0].mergedMembers.map(\.calendarKey) == (0..<5).map { "fake/c\($0)" })
    #expect(result[0].additionalCalendarKeys.count == 4)
    #expect(result[0].mergeProvenance == .rule)
}

@Test func modelMergesJoinAtMostFourClustersAndTheRestStaySeparate() {
    // Five distinct entities that only the model links: the cap limits clusters, not certain duplicates.
    let titles = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]
    let events = titles.enumerated().map { makeEvent("e\($0.offset)", title: $0.element, calendarID: "c\($0.offset)") }
    let verdicts = cache(answering: .same, for: resolve(events, verdicts: VerdictCache()))
    let result = resolve(events, verdicts: verdicts).events
    #expect(result.count == 2)
    let group = result.first { !$0.mergedMembers.isEmpty }!
    #expect(group.mergedMembers.map(\.calendarKey) == ["fake/c0", "fake/c1", "fake/c2", "fake/c3"])
    #expect(group.mergeProvenance == .inference(engineID: "test", engineName: "Test AI"))
    #expect(result.first { $0.mergedMembers.isEmpty }!.calendarKey == "fake/c4")
}

// MARK: the model judges each pair of groups once

private let apptTitle = "Mando (X1102)'s Upcoming Appointment"
private func appointment(_ n: Int, location: String = "Clinic, 5 Main St") -> TimeTugCalendarEvent {
    makeEvent("x\(n)", title: apptTitle, start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "X\(n)",
              location: location, notes: "Bring your card")
}
private let spem = makeEvent("s", title: "Mando Spem Collection", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "S")

private func permutations<T>(_ items: [T]) -> [[T]] {
    guard items.count > 1 else { return [items] }
    return items.indices.flatMap { i in
        var rest = items
        let head = rest.remove(at: i)
        return permutations(rest).map { [head] + $0 }
    }
}

@Test func spemAndThreeIdenticalCopiesProduceOneRequestNotThree() {
    let x = (1...3).map { appointment($0) }
    for order in permutations([spem] + x) {
        let result = resolve(order, verdicts: VerdictCache())
        #expect(result.pending.count == 1, "order \(order.map(\.sourceEventID))")
        #expect(result.events.count == 2)
    }
}

@Test func oneSameVerdictMergesTheWholeGroupPair() {
    let x = (1...3).map { appointment($0) }
    let verdicts = cache(answering: .same, for: resolve([spem] + x, verdicts: VerdictCache()))
    for order in permutations([spem] + x) {
        let result = resolve(order, verdicts: verdicts)
        #expect(result.events.count == 1, "order \(order.map(\.sourceEventID))")
        #expect(result.pending.isEmpty)
        guard let card = result.events.first else { continue }
        #expect(card.mergedMembers.count == 4)
        #expect(card.mergeProvenance == .inference(engineID: "test", engineName: "Test AI"))
        #expect(Set(card.allCalendarKeys) == ["fake/S", "fake/X1", "fake/X2", "fake/X3"])
    }
}

@Test func oneDifferentVerdictKeepsSpemApartAndTheCopiesMerged() {
    let x = (1...3).map { appointment($0) }
    let verdicts = cache(answering: .different, for: resolve([spem] + x, verdicts: VerdictCache()))
    let result = resolve([spem] + x, verdicts: verdicts)
    #expect(result.events.count == 2)
    #expect(result.pending.isEmpty)
    #expect(result.events.contains { $0.mergedMembers.count == 3 && $0.mergeProvenance == .rule })
    #expect(result.events.contains { $0.mergedMembers.isEmpty && $0.calendarKey == "fake/S" })
}

@Test func requestIDDoesNotDependOnWhichIdenticalCopyRepresents() {
    let x = (1...3).map { appointment($0) }
    let ids = permutations([spem] + x).map { resolve($0, verdicts: VerdictCache()).pending.map(\.id) }
    #expect(Set(ids.flatMap { $0 }).count == 1)
    // Same content on differently keyed calendars gives the same id.
    let other = makeEvent("z", title: apptTitle, start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "Z9",
                          location: "Clinic, 5 Main St", notes: "Bring your card")
    #expect(resolve([spem, other], verdicts: VerdictCache()).pending.map(\.id) == ids[0])
}

@Test func anEditedEventGetsANewRequestID() {
    let before = resolve([spem, appointment(1)], verdicts: VerdictCache()).pending.map(\.id)
    let after = resolve([spem, appointment(1, location: "Other Clinic, 9 Elm St")], verdicts: VerdictCache()).pending.map(\.id)
    #expect(before.count == 1 && after.count == 1)
    #expect(before != after)
}

@Test func aVerdictOnAMergedGroupPairLeavesNewNeighboursPendingOnTheNextPass() {
    // Spem + copies merge by "same"; a further unrelated look-alike then needs its own request.
    let x = (1...2).map { appointment($0) }
    let extra = makeEvent("q", title: "Something else", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "Q")
    let first = resolve([spem] + x + [extra], verdicts: VerdictCache())
    var verdicts = VerdictCache()
    let spemRequest = first.pending.first { Set([$0.first.title, $0.second.title]) == [apptTitle, spem.title] }!
    verdicts.store(AdjudicationVerdict(requestID: spemRequest.id, answer: .same), engine: engine,
                   end: t0.addingTimeInterval(86_400), now: t0)
    let second = resolve([spem] + x + [extra], verdicts: verdicts)
    #expect(second.events.count == 2)
    #expect(second.pending.count == 1)
}

@Test func aLessonDifferentIsAHardBlockThatVerdictsCannotOverride() {
    let x = (1...3).map { appointment($0) }
    var lessons = LessonBook()
    lessons.record(MergedMember(spem), MergedMember(x[2]), decision: .different, now: t0)
    let pending = resolve([spem] + x, lessons: lessons, verdicts: VerdictCache()).pending
    #expect(pending.isEmpty)   // blocked group pair is never asked about
    let result = resolve([spem] + x, lessons: lessons, verdicts: VerdictCache())
    #expect(result.events.count == 2)
    #expect(result.events.contains { $0.mergedMembers.count == 3 && $0.mergeProvenance == .rule })
}

@Test func sixExactDuplicatesPlusAPlaceholderIsTwoClustersAndMerges() {
    let x = (1...6).map { appointment($0) }
    let verdicts = cache(answering: .same, for: resolve([spem] + x, verdicts: VerdictCache()))
    let result = resolve([spem] + x, verdicts: verdicts).events
    #expect(result.count == 1)
    #expect(result[0].mergedMembers.count == 7)
    #expect(result[0].additionalCalendarKeys.count == 6)
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

private func mergedEvent(_ events: [TimeTugCalendarEvent]) -> TimeTugCalendarEvent {
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

// MARK: - Three literal phases

@Test func resolveMergesExactCopiesBeforeOtherRulesAndIsOrderIndependent() {
    // Three identical copies; only one carries the conference link that ties in D, and D's location
    // conflicts with another copy. Exact copies must join first, so D stays out for every input order.
    let a = makeEvent("a", title: "Sync", start: "2026-09-18T13:00:00Z", calendarID: "a", notes: "https://acme.zoom.us/j/555")
    let b = makeEvent("b", title: "Sync", start: "2026-09-18T13:00:00Z", calendarID: "b", location: "Room 1")
    let c = makeEvent("c", title: "Sync", start: "2026-09-18T13:00:00Z", calendarID: "c")
    let d = makeEvent("d", title: "Sync call", start: "2026-09-18T13:25:00Z", calendarID: "d",
                      location: "Room 9", notes: "https://acme.zoom.us/j/555")
    let placeholder = makeEvent("p", title: "Team Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "e", others: 0)
    let base = [a, b, c, d, placeholder]
    let verdicts = cache(answering: .same, for: resolve(base, verdicts: VerdictCache()))

    func memberSets(_ events: [TimeTugCalendarEvent]) -> Set<Set<String>> {
        let result = resolve(events, verdicts: verdicts)
        return Set(result.events.map { Set($0.mergedMembers.isEmpty ? [$0.calendarKey] : $0.mergedMembers.map(\.calendarKey)) })
    }
    let expected: Set<Set<String>> = [["fake/a", "fake/b", "fake/c", "fake/e"], ["fake/d"]]
    let orders: [[TimeTugCalendarEvent]] = [
        base, base.reversed(), [d, c, b, a, placeholder], [placeholder, d, a, c, b],
        [c, placeholder, a, d, b], [b, d, placeholder, c, a],
    ]
    for order in orders { #expect(memberSets(order) == expected) }
}

// MARK: - Display span (longer copy) and tug time (conference copy)

private let zoomLink = URL(string: "https://acme.zoom.us/j/777")!
private let teamsLink = URL(string: "https://teams.microsoft.com/l/meetup-join/abc")!

@Test func mergedEventDisplaysTheLongerCopyButTugsAtTheCarrierStart() {
    let short = makeEvent("1", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "a", conferenceURL: zoomLink)
    let long = makeEvent("2", title: "Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "b", conferenceURL: zoomLink)
    let merged = resolve([short, long]).events[0]
    #expect(merged.mergedMembers.count == 2)
    #expect(merged.start == date("2026-09-18T13:00:00Z"))      // primary (earliest on a tie) carries the link
    #expect(merged.displayStart == date("2026-09-18T12:45:00Z"))
    #expect(merged.shownStart == date("2026-09-18T12:45:00Z"))
    #expect(merged.end == date("2026-09-18T13:45:00Z"))
}

@Test func equalDurationsKeepThePrimaryAsTheLongerCopy() {
    let a = makeEvent("1", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "a", conferenceURL: zoomLink)
    let b = makeEvent("2", title: "Sync", start: "2026-09-18T13:05:00Z", minutes: 30, calendarID: "b", conferenceURL: zoomLink)
    let merged = resolve([a, b]).events[0]
    #expect(merged.start == date("2026-09-18T13:00:00Z"))
    #expect(merged.end == date("2026-09-18T13:30:00Z"))
    #expect(merged.displayStart == nil)
    #expect(merged.shownStart == merged.start)
}

@Test func unmergedEventIsUnchanged() {
    let event = makeEvent("1", start: "2026-09-18T13:00:00Z", conferenceURL: zoomLink)
    let result = resolve([event]).events[0]
    #expect(result == event)
    #expect(result.displayStart == nil)
}

@Test func onlyANonPrimaryCopyHasTheLinkSoItCarriesTheTug() {
    let rich = makeEvent("1", title: "Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "a",
                         location: "Room 1", notes: "Agenda", externalUID: "u")
    let linked = makeEvent("2", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "b",
                           conferenceURL: zoomLink, externalUID: "u")
    let merged = resolve([rich, linked]).events[0]
    #expect(merged.calendarKey == "fake/a")
    #expect(merged.start == date("2026-09-18T13:00:00Z"))
    #expect(merged.displayStart == date("2026-09-18T12:45:00Z"))
    #expect(merged.end == date("2026-09-18T13:45:00Z"))
}

@Test func twoNonPrimaryCarriersTugAtTheEarliestOne() {
    let rich = makeEvent("1", title: "Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "a",
                         location: "Room 1", notes: "Agenda", externalUID: "u")
    let late = makeEvent("2", title: "Sync", start: "2026-09-18T13:10:00Z", minutes: 20, calendarID: "b",
                         conferenceURL: zoomLink, externalUID: "u")
    let early = makeEvent("3", title: "Sync", start: "2026-09-18T13:05:00Z", minutes: 25, calendarID: "c",
                          conferenceURL: teamsLink, externalUID: "u")
    let merged = resolve([rich, late, early]).events[0]
    #expect(merged.mergedMembers.count == 3)
    #expect(merged.start == date("2026-09-18T13:05:00Z"))
    #expect(merged.displayStart == date("2026-09-18T12:45:00Z"))
}

@Test func noLinkTugsAtTheLongerCopyStart() {
    let short = makeEvent("1", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "a", externalUID: "u")
    let long = makeEvent("2", title: "Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "b", externalUID: "u")
    let merged = resolve([short, long]).events[0]
    #expect(merged.start == date("2026-09-18T12:45:00Z"))
    #expect(merged.displayStart == nil)
    #expect(merged.end == date("2026-09-18T13:45:00Z"))
}

@Test func barePlaceholderWithThreeIdenticalCopiesShowsThePlaceholderRange() {
    let placeholder = makeEvent("p", title: "Team Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "p", others: 0)
    let copies = ["a", "b", "c"].map {
        makeEvent($0, title: "Sync", start: "2026-09-18T13:00:00Z", calendarID: $0, location: "Room 1")
    }
    let events = [placeholder] + copies
    let merged = resolve(events, verdicts: cache(answering: .same, for: resolve(events, verdicts: VerdictCache()))).events
    #expect(merged.count == 1)
    #expect(merged[0].mergedMembers.count == 4)
    #expect(merged[0].start == date("2026-09-18T12:45:00Z"))
    #expect(merged[0].end == date("2026-09-18T13:45:00Z"))
    #expect(merged[0].title == "Sync")
    #expect(merged[0].location == "Room 1")
    #expect(merged[0].displayStart == nil)
}

@Test func conferenceAppointmentTugsAtItsStartWhileTheListShowsThePlaceholderRange() {
    let placeholder = makeEvent("p", title: "Team Sync", start: "2026-09-18T12:30:00Z", minutes: 60, calendarID: "other", others: 0)
    let appointment = makeEvent("a", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "cal",
                                notes: "Join https://acme.zoom.us/j/777")
    let events = [placeholder, appointment]
    let merged = resolve(events, verdicts: cache(answering: .same, for: resolve(events, verdicts: VerdictCache()))).events
    #expect(merged.count == 1)
    let event = merged[0]
    #expect(event.displayStart == date("2026-09-18T12:30:00Z"))
    #expect(event.start == date("2026-09-18T13:00:00Z"))
    #expect(event.end == date("2026-09-18T13:30:00Z"))
    #expect(event.allContentKeys.isSuperset(of: [placeholder.contentKey, appointment.contentKey]))

    // The takeover follows the tug start, never the display start.
    let next = Scheduler.next(events: merged, settings: optedIn { $0.leadTime = 120 },
                              ledger: TakeoverLedger(), now: date("2026-09-18T12:00:00Z"))
    #expect(next?.fireAt == date("2026-09-18T12:58:00Z"))

    // A fired member counts for the merged card.
    var ledger = TakeoverLedger()
    ledger.markFired(appointment, now: recordedAt)
    #expect(ledger.hasFired(event))
}

@Test func mergedNoLinkEventFiresAtTheLongerCopyStart() {
    let short = makeEvent("1", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "cal", externalUID: "u")
    let long = makeEvent("2", title: "Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "other", externalUID: "u")
    let merged = resolve([short, long]).events
    let next = Scheduler.next(events: merged, settings: optedIn { $0.leadTime = 120 },
                              ledger: TakeoverLedger(), now: date("2026-09-18T12:00:00Z"))
    #expect(next?.fireAt == date("2026-09-18T12:43:00Z"))
}

@Test func requestCarriesRuleComputedFacts() {
    // doctor 10:00-11:00 (bare) vs official 10:00-11:00 (location+notes): richer copy is first.
    let a = makeEvent("1", title: "Scott: Doctor", start: "2026-09-18T10:00:00Z", minutes: 60, calendarID: "p", others: 0)
    let b = makeEvent("2", title: "Intermountain", start: "2026-09-18T10:15:00Z", minutes: 30, calendarID: "w", others: 0,
                      location: "1234 Main St", notes: "Bring card")
    let request = resolve([a, b], verdicts: VerdictCache()).pending[0]
    #expect(request.first.title == "Intermountain" && request.second.title == "Scott: Doctor")
    #expect(request.startOffsetMinutes == -15)   // second (10:00) minus first (10:15)
    #expect(request.endOffsetMinutes == 15)      // 11:00 minus 10:45
    #expect(request.overlapMinutes == 30)
    #expect(request.firstDetails == "location+notes")
    #expect(request.secondDetails == "bare")
    #expect(request.hasConflictingDetails == false)
}

@Test func mergedMembersCarryEachCopysOwnTimes() {
    let short = makeEvent("1", title: "Sync", start: "2026-09-18T13:00:00Z", minutes: 30, calendarID: "a")
    let placeholder = makeEvent("2", title: "Sync", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "b", others: 0)
    let merged = resolve([short, placeholder], lessons: {
        var book = LessonBook()
        book.record(MergedMember(short), MergedMember(placeholder), decision: .same, now: t0)
        return book
    }()).events[0]
    let byCalendar = Dictionary(uniqueKeysWithValues: merged.mergedMembers.map { ($0.calendarKey, $0) })
    #expect(byCalendar["fake/a"]?.start == date("2026-09-18T13:00:00Z"))
    #expect(byCalendar["fake/a"]?.end == date("2026-09-18T13:30:00Z"))
    #expect(byCalendar["fake/b"]?.start == date("2026-09-18T12:45:00Z"))
    #expect(byCalendar["fake/b"]?.end == date("2026-09-18T13:45:00Z"))
}
