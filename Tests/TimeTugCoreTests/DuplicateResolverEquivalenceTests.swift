import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private let equivalenceNow = date("2026-09-18T09:00:00Z")
private let equivalenceEngine = EngineInfo(id: "equivalence", displayName: "Equivalence model", isOnDevice: true)

private func checkEquivalent(_ events: [TimeTugCalendarEvent], lessons: LessonBook = LessonBook()) {
    for inferenceOn in [false, true] {
        let empty: VerdictCache? = inferenceOn ? VerdictCache() : nil
        let baseline = BaselineDuplicateResolver.resolve(events: events, calendars: [], lessons: lessons, verdicts: empty)
        let actual = DuplicateResolver.resolve(events: events, calendars: [], lessons: lessons, verdicts: empty)
        #expect(actual.events == baseline.events)
        #expect(actual.pending == baseline.pending)
        #expect(actual.candidates == baseline.candidates)
        #expect(actual.usedLessonKeys == baseline.usedLessonKeys)

        guard inferenceOn else { continue }
        var verdicts = VerdictCache()
        for (index, request) in baseline.pending.enumerated() {
            let answer: AdjudicationVerdict.Answer = index.isMultiple(of: 3) ? .same : (index.isMultiple(of: 2) ? .different : .unsure)
            verdicts.store(AdjudicationVerdict(requestID: request.id, answer: answer), engine: equivalenceEngine,
                           end: equivalenceNow.addingTimeInterval(86_400), now: equivalenceNow)
        }
        let decidedBaseline = BaselineDuplicateResolver.resolve(events: events, calendars: [], lessons: lessons, verdicts: verdicts)
        let decidedActual = DuplicateResolver.resolve(events: events, calendars: [], lessons: lessons, verdicts: verdicts)
        #expect(decidedActual.events == decidedBaseline.events)
        #expect(decidedActual.pending == decidedBaseline.pending)
        #expect(decidedActual.candidates == decidedBaseline.candidates)
        #expect(decidedActual.usedLessonKeys == decidedBaseline.usedLessonKeys)
    }
}

@Test func resolverMatchesFrozenOracleOnBoundaryAndLessonFixtures() {
    let zoom = URL(string: "https://acme.zoom.us/j/123")!
    let teams = URL(string: "https://teams.microsoft.com/l/meetup-join/other")!
    let events: [TimeTugCalendarEvent] = [
        makeEvent("exact-a", title: "Shared", calendarID: "a"),
        makeEvent("exact-b", title: "Shared", calendarID: "b"),
        makeEvent("same-cal", title: "Shared", calendarID: "a"),
        makeAllDay("day-a", zone: "UTC", first: day(2026, 9, 18), endExclusive: day(2026, 9, 19), title: "Holiday", calendarID: "a"),
        makeAllDay("day-b", zone: "UTC", first: day(2026, 9, 18), endExclusive: day(2026, 9, 19), title: "Holiday", calendarID: "b"),
        makeEvent("uid-a", title: "UID one", calendarID: "c", externalUID: "common", uidScope: .global),
        makeEvent("uid-b", title: "UID two", calendarID: "d", externalUID: "common", uidScope: .global),
        makeEvent("room-a", title: "Planning", calendarID: "e", location: "Room 101"),
        makeEvent("room-b", title: "Planning", calendarID: "f", location: "Room 202"),
        makeEvent("link-a", title: "Call A", calendarID: "g", conferenceURL: zoom),
        makeEvent("link-b", title: "Call B", calendarID: "h", conferenceURL: teams),
        makeEvent("early", title: "Early", start: "2026-09-18T08:00:00Z", minutes: 30, calendarID: "i"),
        makeEvent("late", title: "Late", start: "2026-09-18T14:00:00Z", minutes: 30, calendarID: "j"),
    ]
    var lessons = LessonBook()
    lessons.record(MergedMember(events[0]), MergedMember(events[1]), decision: .different, now: equivalenceNow)
    lessons.record(MergedMember(events[7]), MergedMember(events[8]), decision: .same, now: equivalenceNow)
    checkEquivalent(events, lessons: lessons)
}

@Test func resolverMatchesFrozenOracleForDeterministicRandomizedInputs() {
    var state: UInt64 = 0x5eed_2026
    func next() -> UInt64 {
        state = state &* 6364136223846793005 &+ 1442695040888963407
        return state
    }
    let starts = ["2026-09-18T08:00:00Z", "2026-09-18T08:20:00Z", "2026-09-18T08:45:00Z",
                  "2026-09-18T09:00:00Z", "2026-09-18T09:30:00Z", "2026-09-18T11:00:00Z"]
    let titles = ["Sync", "Planning", "Standup", "Review", "Holiday"]
    let locations: [String?] = [nil, "Room 101", "Room 202", "Main hall"]
    for fixture in 0..<16 {
        var events: [TimeTugCalendarEvent] = []
        for index in 0..<24 {
            let start = starts[Int(next() % UInt64(starts.count))]
            let title = titles[Int(next() % UInt64(titles.count))]
            let calendar = "c\(next() % 5)"
            let location = locations[Int(next() % UInt64(locations.count))]
            let uid = next() % 7 == 0 ? "uid-\(next() % 4)" : nil
            events.append(makeEvent("f\(fixture)-e\(index)", title: title, start: start,
                                    minutes: [30, 60, 90][Int(next() % 3)], calendarID: calendar,
                                    location: location, externalUID: uid, uidScope: uid == nil ? nil : .global))
        }
        checkEquivalent(events)
    }
}
