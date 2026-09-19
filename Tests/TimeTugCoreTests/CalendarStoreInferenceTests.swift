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
