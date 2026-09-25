import Foundation

/// One user correction. Titles are normalized; no notes, names or emails are kept.
public struct Lesson: Codable, Equatable, Sendable {
    public enum Decision: String, Codable, Sendable { case same, different }

    public var titleA: String
    public var titleB: String
    public var calendarKeyA: String
    public var calendarKeyB: String
    /// Which details each side had ("bare", "location+notes"), for the model's context.
    public var signalsA: String
    public var signalsB: String
    public var decision: Decision
    public var lastUsed: Date
    /// Set only for two different events on the same calendar: the lesson then covers just those two occurrences
    /// (title, start, end), not every later pair with the same titles. Absent (nil) in lessons saved before this
    /// existed and in cross-calendar lessons, which match by title and calendar.
    public var contentKeyA: String? = nil
    public var contentKeyB: String? = nil

    public var pairKey: String {
        LessonBook.pairKey(titleA: titleA, calendarKeyA: calendarKeyA, titleB: titleB, calendarKeyB: calendarKeyB,
                           contentKeyA: contentKeyA, contentKeyB: contentKeyB)
    }
}

/// Small, bounded memory of the user's merge and unmerge decisions.
public struct LessonBook: Codable, Equatable, Sendable {
    public static let maxLessons = 300
    public static let expiry: TimeInterval = 182 * 24 * 60 * 60
    public static let promptLimit = 5

    public private(set) var lessons: [Lesson] = []

    public init() {}

    /// Order-independent identity of a (title, calendar) pair.
    public static func pairKey(
        titleA: String, calendarKeyA: String, titleB: String, calendarKeyB: String,
        contentKeyA: String? = nil, contentKeyB: String? = nil
    ) -> String {
        func side(_ title: String, _ calendarKey: String, _ contentKey: String?) -> String {
            "\(DuplicateRules.normalize(title))|\(calendarKey)" + (contentKey.map { "|" + $0 } ?? "")
        }
        let sides = [side(titleA, calendarKeyA, contentKeyA), side(titleB, calendarKeyB, contentKeyB)].sorted()
        return sides[0] + "#" + sides[1]
    }

    /// Two different events on one calendar: a lesson about them is scoped to these occurrences.
    private static func isScoped(_ a: (calendarKey: String, contentKey: String), _ b: (calendarKey: String, contentKey: String)) -> Bool {
        a.calendarKey == b.calendarKey && a.contentKey != b.contentKey
    }

    public mutating func record(_ a: MergedMember, _ b: MergedMember, decision: Lesson.Decision, now: Date) {
        // Two different events on one calendar are remembered too, but only as those occurrences: a title-level
        // lesson would apply to every later pair with these titles on that calendar.
        let scoped = Self.isScoped((a.calendarKey, a.contentKey), (b.calendarKey, b.contentKey))
        let sides = [a, b].sorted { side($0, scoped: scoped) < side($1, scoped: scoped) }
        let lesson = Lesson(
            titleA: DuplicateRules.normalize(sides[0].title), titleB: DuplicateRules.normalize(sides[1].title),
            calendarKeyA: sides[0].calendarKey, calendarKeyB: sides[1].calendarKey,
            signalsA: sides[0].details, signalsB: sides[1].details, decision: decision, lastUsed: now,
            contentKeyA: scoped ? sides[0].contentKey : nil, contentKeyB: scoped ? sides[1].contentKey : nil)
        lessons.removeAll { $0.pairKey == lesson.pairKey }
        lessons.append(lesson)
        prune(now: now)
    }

    /// The recorded decision for this pair of events, if any.
    public func decision(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> Lesson? {
        let scoped = Self.isScoped((a.calendarKey, a.contentKey), (b.calendarKey, b.contentKey))
        let key = Self.pairKey(titleA: a.title, calendarKeyA: a.calendarKey, titleB: b.title, calendarKeyB: b.calendarKey,
                               contentKeyA: scoped ? a.contentKey : nil, contentKeyB: scoped ? b.contentKey : nil)
        return lessons.first { $0.pairKey == key }
    }

    /// Up to `promptLimit` lessons most similar to this pair (shared title words, same calendar pair).
    public func relevant(to a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> [Lesson] {
        let words = Set(DuplicateRules.normalize(a.title).split(separator: " ") + DuplicateRules.normalize(b.title).split(separator: " "))
        let calendars: Set<String> = [a.calendarKey, b.calendarKey]
        let scored: [(lesson: Lesson, score: Int)] = lessons.map { lesson in
            let lessonWords = Set(lesson.titleA.split(separator: " ") + lesson.titleB.split(separator: " "))
            let calendarBonus = calendars == [lesson.calendarKeyA, lesson.calendarKeyB] ? 2 : 0
            return (lesson, words.intersection(lessonWords).count + calendarBonus)
        }
        return scored.filter { $0.score > 0 }
            .sorted { x, y in
                if x.score != y.score { return x.score > y.score }
                if x.lesson.lastUsed != y.lesson.lastUsed { return x.lesson.lastUsed > y.lesson.lastUsed }
                return x.lesson.pairKey < y.lesson.pairKey
            }
            .prefix(Self.promptLimit).map(\.lesson)
    }

    public mutating func touch(_ pairKeys: Set<String>, now: Date) {
        for index in lessons.indices where pairKeys.contains(lessons[index].pairKey) { lessons[index].lastUsed = now }
    }

    /// Drops lessons unused for `expiry`, then the oldest beyond `maxLessons`.
    public mutating func prune(now: Date) {
        lessons.removeAll { now.timeIntervalSince($0.lastUsed) > Self.expiry }
        if lessons.count > Self.maxLessons {
            lessons.sort { $0.lastUsed < $1.lastUsed }
            lessons.removeFirst(lessons.count - Self.maxLessons)
        }
    }

    private func side(_ member: MergedMember, scoped: Bool) -> String {
        "\(DuplicateRules.normalize(member.title))|\(member.calendarKey)" + (scoped ? "|" + member.contentKey : "")
    }
}
