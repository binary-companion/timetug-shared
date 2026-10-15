import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

private struct Boom: Error {}

private final class FakeTextGenerator: TextGenerator, @unchecked Sendable {
    enum Step: Sendable { case answer(String), fail, hang }
    let availabilityValue = LockedBox<TextGeneratorAvailability>(.available(EngineInfo(id: "fake", displayName: "Fake", isOnDevice: true)))
    private let steps: LockedBox<[Step]>
    let calls = LockedBox<Int>(0)
    var onCall: (@Sendable () -> Void)?

    init(_ steps: [Step]) { self.steps = LockedBox(steps) }

    var availability: TextGeneratorAvailability { get async { availabilityValue.get() } }

    func generate(instructions: String, prompt: String) async throws -> String {
        calls.mutate { $0 += 1 }
        onCall?()
        let step = steps.mutate { $0.isEmpty ? Step.answer("unsure") : $0.removeFirst() }
        switch step {
        case .answer(let text): return text
        case .fail: throw Boom()
        case .hang:
            try await Task.sleep(nanoseconds: 60_000_000_000)
            return "same"
        }
    }
}

private func request(_ id: String) -> AdjudicationRequest {
    let start = Date(timeIntervalSince1970: 1_800_000_000)
    func ev(_ t: String) -> AdjudicationEvent {
        AdjudicationEvent(TimeTugCalendarEvent(
            event: CalendarCore.CalendarEvent(eventID: t, calendarID: "c", title: t, start: start, end: start.addingTimeInterval(3600)),
            sourceID: "s"), calendar: nil)
    }
    return AdjudicationRequest(id: id, first: ev("A"), second: ev("B"), lessons: [])
}

private func make(_ generator: FakeTextGenerator, clock: ManualClock? = nil,
                  log: LockedBox<[EngineDiagnostic]>? = nil) -> TextGeneratorAdjudicator {
    TextGeneratorAdjudicator(
        generator: generator, clock: clock ?? ManualClock(start: Date(timeIntervalSince1970: 1_800_000_000)),
        timeZone: { TimeZone(identifier: "UTC")! }, diagnostics: { e in log?.mutate { $0.append(e) } })
}

@Suite struct TextGeneratorAdjudicatorTests {
    @Test func parsesAnswersTrimmingAndLowercasing() async {
        let gen = FakeTextGenerator([.answer("same"), .answer("  Different\n"), .answer("UNSURE ")])
        let verdicts = await make(gen).judge([request("1"), request("2"), request("3")])
        #expect(verdicts == [AdjudicationVerdict(requestID: "1", answer: .same),
                             AdjudicationVerdict(requestID: "2", answer: .different),
                             AdjudicationVerdict(requestID: "3", answer: .unsure)])
    }

    @Test func unparseableAnswerSkipsOnlyThatRequest() async {
        let gen = FakeTextGenerator([.answer("maybe"), .answer("same")])
        let verdicts = await make(gen).judge([request("1"), request("2")])
        #expect(verdicts == [AdjudicationVerdict(requestID: "2", answer: .same)])
    }

    @Test func throwingGeneratorYieldsNoVerdictAndLogsOnlyTheTypeName() async {
        let log = LockedBox<[EngineDiagnostic]>([])
        let gen = FakeTextGenerator([.fail, .answer("same")])
        let verdicts = await make(gen, log: log).judge([request("1"), request("2")])
        #expect(verdicts == [AdjudicationVerdict(requestID: "2", answer: .same)])
        let events = log.get()
        #expect(events.count == 1)
        #expect(events[0].message.contains("Boom"))
        #expect(!events[0].message.contains("\"A\""))
    }

    @Test func temporarilyUnavailableReturnsNothingWithoutCallingTheGenerator() async {
        let gen = FakeTextGenerator([.answer("same")])
        gen.availabilityValue.set(.temporarilyUnavailable(reason: "energy saver"))
        let adjudicator = make(gen)
        #expect(await adjudicator.judge([request("1")]).isEmpty)
        #expect(gen.calls.get() == 0)
        #expect(adjudicator.availability == .unavailable(reason: "temporary: energy saver"))
    }

    @Test func permanentUnavailabilityKeepsItsReasonAndAvailableIsCached() async {
        let gen = FakeTextGenerator([])
        let adjudicator = make(gen)
        #expect(adjudicator.availability == .unavailable(reason: "not checked yet"))
        gen.availabilityValue.set(.unavailable(reason: "model not ready"))
        await adjudicator.refreshAvailability()
        #expect(adjudicator.availability == .unavailable(reason: "model not ready"))
        let info = EngineInfo(id: "fake", displayName: "Fake", isOnDevice: true)
        gen.availabilityValue.set(.available(info))
        await adjudicator.refreshAvailability()
        #expect(adjudicator.availability == .available(info))
    }

    @Test func atMostTwentyRequestsAreJudged() async {
        let gen = FakeTextGenerator([])
        let verdicts = await make(gen).judge((0..<25).map { request("r\($0)") })
        #expect(verdicts.count == 20)
        #expect(gen.calls.get() == 20)
    }

    @Test func deadlineUsesTheInjectedClock() async {
        let clock = ManualClock(start: Date(timeIntervalSince1970: 1_800_000_000))
        let gen = FakeTextGenerator([])
        // The first call "takes" 31 seconds on the manual clock; the loop must stop before the second.
        gen.onCall = { [clock] in clock.advanceNow(by: 31) }
        let verdicts = await make(gen, clock: clock).judge([request("1"), request("2"), request("3")])
        #expect(verdicts.count == 1)
        #expect(gen.calls.get() == 1)
    }

    @Test func cancellationStopsTheLoop() async {
        let gen = FakeTextGenerator([.hang, .answer("same")])
        let adjudicator = make(gen)
        let task = Task { await adjudicator.judge([request("1"), request("2")]) }
        await poll("the first generate call") { gen.calls.get() > 0 }
        task.cancel()
        let verdicts = await task.value
        #expect(verdicts.isEmpty)
        #expect(gen.calls.get() == 1)
    }
}
