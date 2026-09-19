import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T09:00:00Z")

final class FakeAdjudicator: DuplicateAdjudicator, @unchecked Sendable {
    static let engine = EngineInfo(id: "fake-ai", displayName: "Fake AI", isOnDevice: true)
    let availability: AdjudicatorAvailability
    private let answer: AdjudicationVerdict.Answer
    private let lock = NSLock()
    private var seen: [AdjudicationRequest] = []

    init(availability: AdjudicatorAvailability = .available(FakeAdjudicator.engine), answer: AdjudicationVerdict.Answer = .same) {
        self.availability = availability
        self.answer = answer
    }

    var requests: [AdjudicationRequest] { lock.withLock { seen } }

    func judge(_ requests: [AdjudicationRequest]) async -> [AdjudicationVerdict] {
        lock.withLock { seen += requests }
        return requests.map { AdjudicationVerdict(requestID: $0.id, answer: answer) }
    }
}

private let doctor = makeEvent("1", title: "Scott: Doctor", minutes: 60, calendarID: "personal", others: 0)
private let official = makeEvent("2", title: "Intermountain Health", minutes: 60, calendarID: "work",
                                 location: "1234 Main St", notes: "Bring insurance card")

private func makeStore(_ adjudicator: FakeAdjudicator?) async -> CalendarStore {
    let source = FakeSource()
    await source.set(events: .success([doctor, official]))
    return CalendarStore(sources: [source], calendar: utcCalendar, adjudicator: adjudicator)
}

@Test func inferenceIsOffByDefaultAndTheEngineIsNeverCalled() async {
    let engine = FakeAdjudicator()
    let store = await makeStore(engine)
    #expect(await store.inferenceStatus() == .disabled)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 2)
    #expect(await store.resolvePending(now: now) == nil)
    #expect(engine.requests.isEmpty)
}

@Test func enabledEngineMergesAfterResolvePendingWithoutBlockingRefresh() async {
    let engine = FakeAdjudicator()
    let store = await makeStore(engine)
    _ = await store.setInferenceEnabled(true, now: now)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 2)   // refresh never waits on the model
    let merged = await store.resolvePending(now: now)
    #expect(merged?.events.count == 1)
    #expect(merged?.events.first?.mergeProvenance == .inference(engineID: "fake-ai", engineName: "Fake AI"))
    #expect(engine.requests.count == 1)
    #expect(await store.resolvePending(now: now) == nil)                       // cached: no second call
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 1)     // cache applied on refresh
}

@Test func unavailableOrOffDeviceEnginesFallBackToRulesOnly() async {
    let unavailable = FakeAdjudicator(availability: .unavailable(reason: "not eligible"))
    var store = await makeStore(unavailable)
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    #expect(await store.inferenceStatus() == .unavailable(reason: "not eligible"))
    #expect(await store.resolvePending(now: now) == nil)
    #expect(unavailable.requests.isEmpty)

    let cloud = FakeAdjudicator(availability: .available(EngineInfo(id: "cloud", displayName: "Cloud", isOnDevice: false)))
    store = await makeStore(cloud)
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    #expect(await store.inferenceStatus() == .notOnDevice)
    #expect(await store.resolvePending(now: now) == nil)
    #expect(cloud.requests.isEmpty)

    let none = await makeStore(nil)
    _ = await none.setInferenceEnabled(true, now: now)
    #expect(await none.inferenceStatus() == .noEngine)
}

@Test func turningInferenceOffIgnoresCachedVerdicts() async {
    let store = await makeStore(FakeAdjudicator())
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    _ = await store.resolvePending(now: now)
    #expect(await store.setInferenceEnabled(false, now: now).events.count == 2)
}

@Test func unmergeSplitsAndRemembersAndManualMergeJoinsAgain() async {
    let store = await makeStore(FakeAdjudicator())
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    let merged = await store.resolvePending(now: now)!.events[0]

    let split = await store.unmerge(merged, now: now)
    #expect(split.events.count == 2)
    #expect(await store.state().lessons.lessons.first?.decision == .different)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 2)     // survives refresh, engine not re-asked

    let a = split.events.first { $0.title == "Scott: Doctor" }!
    let b = split.events.first { $0.title == "Intermountain Health" }!
    let joined = await store.merge(a, b, now: now)
    #expect(joined.events.count == 1)
    #expect(joined.events[0].mergeProvenance == .userConfirmed)
}

@Test func lessonsWorkWithInferenceOffAndCanBeForgotten() async {
    let store = await makeStore(nil)
    let snapshot = await store.refresh(now: now, leadTime: 60)
    let joined = await store.merge(snapshot.events[0], snapshot.events[1], now: now)
    #expect(joined.events.count == 1)
    #expect(await store.forgetLessons(now: now).events.count == 2)
}

@Test func stateRoundTripsThroughLoad() async {
    let store = await makeStore(FakeAdjudicator())
    let snapshot = await store.refresh(now: now, leadTime: 60)
    _ = await store.merge(snapshot.events[0], snapshot.events[1], now: now)
    let state = await store.state()

    let fresh = await makeStore(nil)
    await fresh.load(state)
    #expect(await fresh.refresh(now: now, leadTime: 60).events.count == 1)
}

final class SlowAdjudicator: DuplicateAdjudicator, @unchecked Sendable {
    let availability: AdjudicatorAvailability = .available(FakeAdjudicator.engine)
    private let lock = NSLock()
    private var calls = 0
    var callCount: Int { lock.withLock { calls } }

    func judge(_ requests: [AdjudicationRequest]) async -> [AdjudicationVerdict] {
        lock.withLock { calls += 1 }
        try? await Task.sleep(nanoseconds: 50_000_000)
        return requests.map { AdjudicationVerdict(requestID: $0.id, answer: .same) }
    }
}

@Test func concurrentResolvePendingRunsOnlyOnePass() async {
    let slow = SlowAdjudicator()
    let source = FakeSource()
    await source.set(events: .success([doctor, official]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar, adjudicator: slow)
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    async let first = store.resolvePending(now: now)
    async let second = store.resolvePending(now: now)
    let results = await [first, second]
    #expect(slow.callCount == 1)
    #expect(results.compactMap { $0 }.count == 1)
}

@Test func loadedStateIsPrunedOnTheNextSnapshot() async {
    var stale = VerdictCache()
    stale.store(AdjudicationVerdict(requestID: "old", answer: .same), engine: FakeAdjudicator.engine,
                end: now.addingTimeInterval(-3600), now: now.addingTimeInterval(-VerdictCache.retention - 60))
    #expect(stale.entry(for: "old") != nil)
    let store = await makeStore(nil)
    await store.load(DedupState(verdicts: stale))
    _ = await store.refresh(now: now, leadTime: 60)
    #expect(await store.state().verdicts.entry(for: "old") == nil)
}

@Test func aPairThatAlreadyEndedTodayIsJudgedOnce() async {
    let engine = FakeAdjudicator()
    let store = await makeStore(engine)
    let later = date("2026-09-18T15:00:00Z")   // both events ended hours ago but are still in today's window
    _ = await store.setInferenceEnabled(true, now: later)
    #expect(await store.refresh(now: later, leadTime: 60).events.count == 2)
    var passes = 0
    while await store.resolvePending(now: later) != nil, passes < 5 { passes += 1 }
    #expect(passes == 1)
    #expect(await store.resolvePending(now: later) == nil)
    #expect(await store.refresh(now: later, leadTime: 60).events.count == 1)
    #expect(await store.refresh(now: later, leadTime: 60).events.count == 1)
    #expect(engine.requests.count == 1)
}

private func unmergeExactDuplicates(_ a: CalendarEvent, _ b: CalendarEvent) async {
    let source = FakeSource()
    await source.set(events: .success([a, b]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let merged = await store.refresh(now: now, leadTime: 60)
    #expect(merged.events.count == 1)
    let split = await store.unmerge(merged.events[0], now: now)
    #expect(split.events.count == 2)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 2)   // persists across refresh
    // Undo stays available: each half offers the other as a manual "Merge with".
    let ids = split.events.map(\.id)
    for event in split.events {
        #expect(split.candidates[event.id]?.map(\.id) == ids.filter { $0 != event.id })
    }
    let rejoined = await store.merge(split.events[0], split.events[1], now: now)
    #expect(rejoined.events.count == 1)
}

@Test func unmergingAllDayDuplicatesOnTwoCalendarsSticks() async {
    await unmergeExactDuplicates(
        makeEvent("1", title: "Holiday", calendarID: "personal", isAllDay: true),
        makeEvent("2", title: "Holiday", calendarID: "work", isAllDay: true))
}

@Test func unmergingSameCalendarExactDuplicatesSticks() async {
    await unmergeExactDuplicates(
        makeEvent("1", title: "Standup", calendarID: "work"),
        makeEvent("2", title: "Standup", calendarID: "work"))
}

@Test func forgettingLessonsAlsoForgetsCachedVerdicts() async {
    let engine = FakeAdjudicator()
    let store = await makeStore(engine)
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    #expect(await store.resolvePending(now: now)?.events.count == 1)
    let forgotten = await store.forgetLessons(now: now)
    #expect(forgotten.events.count == 2)                                  // not silently re-merged from cache
    #expect(await store.state().verdicts.entries.isEmpty)
    #expect(await store.resolvePending(now: now)?.events.count == 1)      // pending again, judged again
    #expect(engine.requests.count == 2)
}

@Test func aBacklogLargerThanOnePassDrainsAcrossPasses() async {
    // 25 look-alike pairs, 55 minutes apart so different pairs never fall inside each other's time gate.
    let base = date("2026-09-18T00:10:00Z")
    var events: [CalendarEvent] = []
    for k in 0..<25 {
        let start = base.addingTimeInterval(TimeInterval(k * 55 * 60))
        for (calendarID, title) in [("personal", "Errand \(k)"), ("work", "Visit \(k)")] {
            events.append(CalendarEvent(sourceEventID: "\(calendarID)\(k)", sourceID: "fake", calendarID: calendarID,
                                        title: title, start: start, end: start.addingTimeInterval(20 * 60)))
        }
    }
    let engine = FakeAdjudicator()
    let source = FakeSource()
    await source.set(events: .success(events))
    let store = CalendarStore(sources: [source], calendar: utcCalendar, adjudicator: engine)
    _ = await store.setInferenceEnabled(true, now: now)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 50)
    var passes = 0
    var latest: CalendarSnapshot?
    while let next = await store.resolvePending(now: now), passes < 10 { latest = next; passes += 1 }
    #expect(passes == 2)                       // 20 then 5
    #expect(latest?.events.count == 25)
    #expect(engine.requests.count == 25)
}

@Test func threeIdenticalCopiesAndAPlaceholderAreJudgedOncePerGroupPair() async {
    func copy(_ n: Int) -> CalendarEvent {
        makeEvent("x\(n)", title: "Mando's Upcoming Appointment", start: "2026-09-18T13:00:00Z", minutes: 30,
                  calendarID: "X\(n)", location: "Clinic, 5 Main St", notes: "Bring your card")
    }
    let spem = makeEvent("s", title: "Mando Spem Collection", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "S")
    let engine = FakeAdjudicator()
    let source = FakeSource()
    await source.set(events: .success([spem, copy(1), copy(2), copy(3)]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar, adjudicator: engine)
    _ = await store.setInferenceEnabled(true, now: now)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 2)
    let merged = await store.resolvePending(now: now)
    #expect(engine.requests.count == 1)
    #expect(merged?.events.count == 1)
    #expect(merged?.events.first?.mergedMembers.count == 4)
    #expect(await store.resolvePending(now: now) == nil)
}

// MARK: - Separating one member of a merged card

private func placeholderAndCopies(_ engine: FakeAdjudicator) async -> (CalendarStore, CalendarEvent) {
    func copy(_ n: Int) -> CalendarEvent {
        makeEvent("x\(n)", title: "Mando's Upcoming Appointment", start: "2026-09-18T13:00:00Z", minutes: 30,
                  calendarID: "X\(n)", location: "Clinic, 5 Main St", notes: "Bring your card")
    }
    let spem = makeEvent("s", title: "Mando Spem Collection", start: "2026-09-18T12:45:00Z", minutes: 60, calendarID: "S")
    let source = FakeSource()
    await source.set(events: .success([spem, copy(1), copy(2), copy(3)]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar, adjudicator: engine)
    _ = await store.setInferenceEnabled(true, now: now)
    _ = await store.refresh(now: now, leadTime: 60)
    let merged = await store.resolvePending(now: now)!.events[0]
    return (store, merged)
}

@Test func separatingThePlaceholderLeavesTheCopiesMergedAndPersists() async {
    let (store, merged) = await placeholderAndCopies(FakeAdjudicator())
    #expect(merged.mergedMembers.count == 4)
    let placeholder = merged.mergedMembers.first { $0.calendarKey == "fake/S" }!
    let split = await store.separate(placeholder, from: merged, now: now)
    #expect(split.events.count == 2)
    let copies = split.events.first { $0.mergedMembers.count == 3 }
    #expect(copies != nil)
    #expect(split.events.contains { $0.title == "Mando Spem Collection" && $0.mergedMembers.isEmpty })
    let after = await store.refresh(now: now, leadTime: 60)
    #expect(after.events.count == 2)
    #expect(after.events.contains { $0.mergedMembers.count == 3 })
    #expect(await store.state().lessons.lessons.count == 3)   // placeholder vs each of the three copies
    // The split-off event can be offered for a manual re-merge.
    let alone = after.events.first { $0.mergedMembers.isEmpty }!
    #expect(after.candidates[alone.id]?.isEmpty == false)
}

@Test func separatingOneIdenticalCopyLeavesTheOthersMerged() async {
    let (store, merged) = await placeholderAndCopies(FakeAdjudicator())
    let one = merged.mergedMembers.first { $0.calendarKey == "fake/X1" }!
    let split = await store.separate(one, from: merged, now: now)
    #expect(split.events.count == 2)
    let byMembers = Dictionary(grouping: split.events, by: { Set($0.participants.map(\.calendarKey)) })
    #expect(byMembers[["fake/X1"]]?.count == 1)
    #expect(byMembers[["fake/S", "fake/X2", "fake/X3"]]?.count == 1)
    let after = await store.refresh(now: now, leadTime: 60)
    #expect(Set(after.events.map { Set($0.participants.map(\.calendarKey)) }) == [["fake/X1"], ["fake/S", "fake/X2", "fake/X3"]])
    let alone = after.events.first { $0.participants.count == 1 }!
    #expect(after.candidates[alone.id]?.isEmpty == false)
}

@Test func unmergingTheWholeCardStillWorksAfterASeparation() async {
    let (store, merged) = await placeholderAndCopies(FakeAdjudicator())
    let placeholder = merged.mergedMembers.first { $0.calendarKey == "fake/S" }!
    let split = await store.separate(placeholder, from: merged, now: now)
    let copies = split.events.first { $0.mergedMembers.count == 3 }!
    #expect(await store.unmerge(copies, now: now).events.count == 4)
}
