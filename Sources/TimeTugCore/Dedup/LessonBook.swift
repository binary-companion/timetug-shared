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

    public var pairKey: String { LessonBook.pairKey(titleA: titleA, calendarKeyA: calendarKeyA, titleB: titleB, calendarKeyB: calendarKeyB) }
}

/// Small, bounded memory of the user's merge and unmerge decisions.
public struct LessonBook: Codable, Equatable, Sendable {
    public static let maxLessons = 300
    public static let expiry: TimeInterval = 182 * 24 * 60 * 60
    public static let promptLimit = 5

    public private(set) var lessons: [Lesson] = []

    public init() {}

    /// Order-independent identity of a (title, calendar) pair.
    public static func pairKey(titleA: String, calendarKeyA: String, titleB: String, calendarKeyB: String) -> String {
        let sides = ["\(DuplicateRules.normalize(titleA))|\(calendarKeyA)", "\(DuplicateRules.normalize(titleB))|\(calendarKeyB)"].sorted()
        return sides[0] + "#" + sides[1]
    }

    public mutating func record(_ a: MergedMember, _ b: MergedMember, decision: Lesson.Decision, now: Date) {
        // Same-calendar pairs only matter as exact duplicates (identical content), which rules merge.
        guard a.calendarKey != b.calendarKey || a.contentKey == b.contentKey else { return }
        let sides = [a, b].sorted { side($0) < side($1) }
        let lesson = Lesson(
            titleA: DuplicateRules.normalize(sides[0].title), titleB: DuplicateRules.normalize(sides[1].title),
            calendarKeyA: sides[0].calendarKey, calendarKeyB: sides[1].calendarKey,
            signalsA: sides[0].details, signalsB: sides[1].details, decision: decision, lastUsed: now)
        lessons.removeAll { $0.pairKey == lesson.pairKey }
        lessons.append(lesson)
        prune(now: now)
    }

    /// The recorded decision for this pair of events, if any.
    public func decision(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> Lesson? {
        let key = Self.pairKey(titleA: a.title, calendarKeyA: a.calendarKey, titleB: b.title, calendarKeyB: b.calendarKey)
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

    private func side(_ member: MergedMember) -> String { "\(DuplicateRules.normalize(member.title))|\(member.calendarKey)" }
}
