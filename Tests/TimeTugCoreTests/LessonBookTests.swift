import Foundation
import Testing
@testable import TimeTugCore

private let t0 = date("2026-09-18T09:00:00Z")

private func member(_ title: String, _ calendar: String, _ details: String = "bare") -> MergedMember {
    MergedMember(title: title, calendarKey: calendar, contentKey: title, details: details)
}

@Test func lessonIsLookedUpInEitherOrderByNormalizedTitles() {
    var book = LessonBook()
    book.record(member("Scott: Doctor", "fake/personal"), member("Intermountain Health", "fake/work", "location"), decision: .same, now: t0)
    let a = makeEvent("1", title: "scott doctor!", calendarID: "personal")
    let b = makeEvent("2", title: "INTERMOUNTAIN HEALTH", calendarID: "work")
    #expect(book.decision(a, b)?.decision == .same)
    #expect(book.decision(b, a)?.decision == .same)
}

@Test func sameCalendarPairsAreNotRecorded() {
    var book = LessonBook()
    book.record(member("A", "fake/cal"), member("B", "fake/cal"), decision: .different, now: t0)
    #expect(book.lessons.isEmpty)
}

@Test func newDecisionReplacesTheOldOneForThePair() {
    var book = LessonBook()
    let a = member("A", "fake/x"), b = member("B", "fake/y")
    book.record(a, b, decision: .same, now: t0)
    book.record(b, a, decision: .different, now: t0.addingTimeInterval(60))
    #expect(book.lessons.count == 1)
    #expect(book.lessons.first?.decision == .different)
}

@Test func lessonsAreCappedOldestFirst() {
    var book = LessonBook()
    for i in 0..<(LessonBook.maxLessons + 5) {
        book.record(member("title\(i)", "fake/x"), member("other\(i)", "fake/y"), decision: .same,
                    now: t0.addingTimeInterval(TimeInterval(i)))
    }
    #expect(book.lessons.count == LessonBook.maxLessons)
    #expect(!book.lessons.contains { $0.titleA == "title0" || $0.titleB == "title0" })
}

@Test func unusedLessonsExpireAndTouchRefreshes() {
    var book = LessonBook()
    book.record(member("A", "fake/x"), member("B", "fake/y"), decision: .same, now: t0)
    book.record(member("C", "fake/x"), member("D", "fake/y"), decision: .same, now: t0)
    let key = book.lessons.first { $0.titleA == "a" }!.pairKey
    let later = t0.addingTimeInterval(LessonBook.expiry - 10)
    book.touch([key], now: later)
    book.prune(now: t0.addingTimeInterval(LessonBook.expiry + 10))
    #expect(book.lessons.map(\.titleA) == ["a"])
}

@Test func relevantLessonsPreferSharedWordsAndCalendarsOverRecency() {
    var book = LessonBook()
    book.record(member("Doctor visit", "fake/personal"), member("Clinic", "fake/work"), decision: .same, now: t0)
    book.record(member("Dance", "fake/personal"), member("Studio", "fake/work"), decision: .different, now: t0.addingTimeInterval(500))
    let a = makeEvent("1", title: "Doctor", calendarID: "personal")
    let b = makeEvent("2", title: "Mercy Clinic", calendarID: "work")
    #expect(book.relevant(to: a, b).first?.titleA == "clinic")
}

@Test func codableRoundTrip() throws {
    var book = LessonBook()
    book.record(member("A", "fake/x"), member("B", "fake/y"), decision: .same, now: t0)
    let decoded = try JSONDecoder().decode(LessonBook.self, from: JSONEncoder().encode(book))
    #expect(decoded == book)
}
