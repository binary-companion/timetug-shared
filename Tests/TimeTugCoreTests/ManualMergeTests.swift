import CalendarCore
import Foundation
import Testing
@testable import TimeTugCore

private let t0 = date("2026-09-18T09:00:00Z")

private func resolve(_ events: [TimeTugCalendarEvent], lessons: LessonBook = LessonBook()) -> DuplicateResolution {
    DuplicateResolver.resolve(events: events, calendars: [], lessons: lessons, verdicts: nil)
}

@Test func manualCandidatesMustActuallyOverlap() {
    let a = makeEvent("1", title: "A", start: "2026-09-18T10:00:00Z", minutes: 30, calendarID: "x")
    let touching = makeEvent("2", title: "B", start: "2026-09-18T10:30:00Z", minutes: 30, calendarID: "x")
    let overlapping = makeEvent("3", title: "C", start: "2026-09-18T10:15:00Z", minutes: 60, calendarID: "x")
    let farApart = makeEvent("4", title: "D", start: "2026-09-18T08:00:00Z", minutes: 240, calendarID: "y")
    let allDay = makeAllDay(zone: "UTC", first: day(2026, 9, 18), endExclusive: day(2026, 9, 19), calendarID: "y")
    #expect(!DuplicateRules.isManualCandidate(a, touching))
    #expect(DuplicateRules.isManualCandidate(a, overlapping))       // same calendar is fine
    #expect(DuplicateRules.isManualCandidate(a, farApart))          // starts 2 hours apart is fine
    #expect(!DuplicateRules.isManualCandidate(a, allDay))
}

/// The reported case: a stale 10:00 hold on a subscribed calendar and the real 10:30 consultation, copied onto
/// that calendar and three others. They only touch, so neither row offers "Merge".
@Test func backToBackHoldAndConsultationAreNotOfferedForMerge() {
    let zoom = "https://careerminds.zoom.us/j/78751303881"
    let hold = makeEvent("h", title: "Scott: Careerminds consultation with Michael (tentative)",
                         start: "2026-09-18T10:00:00Z", minutes: 30, calendarID: "shared", others: 0)
    let copies = ["shared", "shared2", "shared3", "google"].enumerated().map { index, calendar in
        makeEvent("c\(index)", title: "Consultation with Michael Munir", start: "2026-09-18T10:30:00Z", minutes: 30,
                  calendarID: calendar, notes: "Join \(zoom)")
    }
    let result = resolve([hold] + copies)
    #expect(result.events.count == 2)
    #expect(result.candidates.isEmpty)
}

@Test func overlappingEventsOnOneCalendarAreOfferedAndAMergeSticks() {
    let a = makeEvent("1", title: "Hold", start: "2026-09-18T10:00:00Z", minutes: 60, calendarID: "cal", others: 0)
    let b = makeEvent("2", title: "Consultation", start: "2026-09-18T10:30:00Z", minutes: 30, calendarID: "cal")
    let before = resolve([a, b])
    #expect(before.events.count == 2)
    #expect(before.candidates[a.id]?.map(\.id) == [b.id])
    #expect(before.candidates[b.id]?.map(\.id) == [a.id])

    var lessons = LessonBook()
    lessons.record(MergedMember(a), MergedMember(b), decision: .same, now: t0)
    let after = resolve([a, b], lessons: lessons)
    #expect(after.events.count == 1)
    #expect(after.events[0].mergeProvenance == .userConfirmed)
}

@Test func aSameCalendarLessonOnlyAppliesToThoseOccurrences() {
    let monday = makeEvent("1", title: "Hold", start: "2026-09-14T10:00:00Z", minutes: 60, calendarID: "cal")
    let mondayCall = makeEvent("2", title: "Call", start: "2026-09-14T10:30:00Z", minutes: 30, calendarID: "cal")
    let tuesday = makeEvent("3", title: "Hold", start: "2026-09-15T10:00:00Z", minutes: 60, calendarID: "cal")
    let tuesdayCall = makeEvent("4", title: "Call", start: "2026-09-15T10:30:00Z", minutes: 30, calendarID: "cal")
    var lessons = LessonBook()
    lessons.record(MergedMember(monday), MergedMember(mondayCall), decision: .same, now: t0)
    #expect(resolve([monday, mondayCall], lessons: lessons).events.count == 1)
    #expect(resolve([tuesday, tuesdayCall], lessons: lessons).events.count == 2)
}

@Test func sameCalendarLessonsForDifferentDaysAreKeptSeparately() {
    let monday = makeEvent("1", title: "Hold", start: "2026-09-14T10:00:00Z", minutes: 60, calendarID: "cal")
    let mondayCall = makeEvent("2", title: "Call", start: "2026-09-14T10:30:00Z", minutes: 30, calendarID: "cal")
    let tuesday = makeEvent("3", title: "Hold", start: "2026-09-15T10:00:00Z", minutes: 60, calendarID: "cal")
    let tuesdayCall = makeEvent("4", title: "Call", start: "2026-09-15T10:30:00Z", minutes: 30, calendarID: "cal")
    var lessons = LessonBook()
    lessons.record(MergedMember(monday), MergedMember(mondayCall), decision: .same, now: t0)
    lessons.record(MergedMember(tuesday), MergedMember(tuesdayCall), decision: .same, now: t0)
    #expect(lessons.lessons.count == 2)
}

@Test func overlappingEventsWithFarApartStartsAreOfferedButNotMergedByRules() {
    let block = makeEvent("1", title: "Workshop", start: "2026-09-18T08:00:00Z", minutes: 240, calendarID: "x")
    let session = makeEvent("2", title: "Workshop", start: "2026-09-18T10:00:00Z", minutes: 30, calendarID: "y")
    let result = resolve([block, session])
    #expect(result.events.count == 2)
    #expect(result.candidates[block.id]?.map(\.id) == [session.id])

    var lessons = LessonBook()
    lessons.record(MergedMember(block), MergedMember(session), decision: .same, now: t0)
    #expect(resolve([block, session], lessons: lessons).events.count == 1)
}

@Test func lessonsSavedBeforeOccurrenceScopingStillDecode() throws {
    let json = """
    {"lessons":[{"titleA":"a","titleB":"b","calendarKeyA":"fake/x","calendarKeyB":"fake/y","signalsA":"bare",
    "signalsB":"bare","decision":"same","lastUsed":0}]}
    """
    let book = try JSONDecoder().decode(LessonBook.self, from: Data(json.utf8))
    #expect(book.lessons.count == 1)
    #expect(book.lessons[0].contentKeyA == nil)
    let a = makeEvent("1", title: "A", calendarID: "x"), b = makeEvent("2", title: "B", calendarID: "y")
    #expect(book.decision(a, b)?.decision == .same)
}

@Test func mergingTheReportedPairThroughTheStoreSticks() async {
    let hold = makeEvent("1", title: "Hold", start: "2026-09-18T10:00:00Z", minutes: 60, calendarID: "shared", others: 0)
    let call = makeEvent("2", title: "Consultation", start: "2026-09-18T10:30:00Z", minutes: 30, calendarID: "shared",
                         conferenceURL: URL(string: "https://acme.zoom.us/j/1")!)
    let source = FakeSource()
    await source.set(events: .success([hold, call]))
    let store = CalendarStore(sources: [source], calendar: utcCalendar)
    let now = date("2026-09-18T09:00:00Z")
    let before = await store.refresh(now: now, leadTime: 60)
    #expect(before.events.count == 2)
    #expect(before.candidates[before.events[0].id]?.count == 1)
    let after = await store.merge(before.events[0], before.events[1], now: now)
    #expect(after.events.count == 1)
    // The tug time follows the copy with the join link.
    #expect(after.events[0].start == call.start)
    #expect(await store.refresh(now: now, leadTime: 60).events.count == 1)
}
