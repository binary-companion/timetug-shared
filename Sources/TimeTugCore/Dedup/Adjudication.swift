import Foundation

public struct EngineInfo: Hashable, Codable, Sendable {
    public var id: String
    /// Product name, e.g. "Apple Intelligence"; the front end words the badge around it.
    public var displayName: String
    public var isOnDevice: Bool

    public init(id: String, displayName: String, isOnDevice: Bool) {
        self.id = id
        self.displayName = displayName
        self.isOnDevice = isOnDevice
    }
}

public enum AdjudicatorAvailability: Equatable, Sendable {
    case available(EngineInfo)
    case unavailable(reason: String)
}

/// The fields of one event a model may see. No emails; notes are truncated.
public struct AdjudicationEvent: Codable, Equatable, Sendable {
    public static let maxTitleLength = 256
    public static let maxLocationLength = 512
    public static let maxCalendarLength = 128
    public static let maxAttendeeNames = 10
    public static let maxAttendeeNameLength = 80
    public static let maxNotesLength = 500

    public var title: String
    public var start: Date
    public var end: Date
    public var location: String?
    public var notes: String?
    public var attendeeNames: [String]
    public var calendarTitle: String?

    public init(_ event: TimeTugCalendarEvent, calendar: CalendarInfo?) {
        title = String(event.title.prefix(Self.maxTitleLength))
        start = event.start
        end = event.end
        location = event.location.map { String($0.prefix(Self.maxLocationLength)) }
        notes = event.notes.map { String($0.prefix(Self.maxNotesLength)) }
        // Some calendar sources put the address in the name field, so drop anything email-shaped.
        attendeeNames = Array(event.attendees.compactMap(\.name).filter { !$0.isEmpty && !$0.contains("@") }
            .prefix(Self.maxAttendeeNames).map { String($0.prefix(Self.maxAttendeeNameLength)) })
        if let calendar { calendarTitle = String(calendar.title.prefix(Self.maxCalendarLength)) }
        else { calendarTitle = nil }
    }

    public func bounded() -> Self {
        var value = self
        value.title = String(title.prefix(Self.maxTitleLength))
        value.location = location.map { String($0.prefix(Self.maxLocationLength)) }
        value.notes = notes.map { String($0.prefix(Self.maxNotesLength)) }
        value.calendarTitle = calendarTitle.map { String($0.prefix(Self.maxCalendarLength)) }
        value.attendeeNames = Array(attendeeNames.filter { !$0.isEmpty && !$0.contains("@") }
            .prefix(Self.maxAttendeeNames).map { String($0.prefix(Self.maxAttendeeNameLength)) })
        return value
    }

    func promptBounded() -> Self {
        var value = bounded()
        value.title = PromptText.bounded(value.title, quotedLimit: 350)
        value.location = value.location.map { PromptText.bounded($0, quotedLimit: 400) }
        value.notes = value.notes.map { PromptText.bounded($0, quotedLimit: 400) }
        value.calendarTitle = value.calendarTitle.map { PromptText.bounded($0, quotedLimit: 160) }
        value.attendeeNames = value.attendeeNames.map { PromptText.bounded($0, quotedLimit: 50) }
        return value
    }
}

/// Shared escaping and length policy for canonical model input and its rendered prompt.
public enum PromptText {
    public static func quoted(_ value: String) -> String {
        let encoder = JSONEncoder()
        return String(decoding: (try? encoder.encode(value)) ?? Data("\"\"".utf8), as: UTF8.self)
            .replacingOccurrences(of: "<", with: "\\u003C")
            .replacingOccurrences(of: ">", with: "\\u003E")
    }

    public static func bounded(_ value: String, quotedLimit: Int) -> String {
        if quoted(value).count <= quotedLimit { return value }
        var prefix = ""
        for character in value {
            let next = prefix + String(character)
            if quoted(next).count > quotedLimit - 3 { break }
            prefix = next
        }
        return prefix + "…"
    }
}

public struct AdjudicationRequest: Equatable, Sendable {
    /// Fingerprint of both events' relevant content (not their calendars); the cache key.
    public var id: String
    /// The more detailed event (ties: the earlier one); the other is `second`.
    public var first: AdjudicationEvent
    public var second: AdjudicationEvent
    public var lessons: [Lesson]
    /// Rule-computed facts about the pair, free of display strings. Offsets are `second` minus `first`.
    public var startOffsetMinutes: Int
    public var endOffsetMinutes: Int
    public var overlapMinutes: Int
    /// `DuplicateRules.detailSummary` of each event, e.g. "bare" or "location+notes".
    public var firstDetails: String
    public var secondDetails: String
    /// False for every ambiguous pair by construction; carried so the prompt can say so.
    public var hasConflictingDetails: Bool

    public var input: JudgmentInput { JudgmentInput(self) }

    public init(id: String, first: AdjudicationEvent, second: AdjudicationEvent, lessons: [Lesson],
                startOffsetMinutes: Int = 0, endOffsetMinutes: Int = 0, overlapMinutes: Int = 0,
                firstDetails: String = "bare", secondDetails: String = "bare", hasConflictingDetails: Bool = false) {
        self.id = id
        self.first = first
        self.second = second
        self.lessons = lessons
        self.startOffsetMinutes = startOffsetMinutes
        self.endOffsetMinutes = endOffsetMinutes
        self.overlapMinutes = overlapMinutes
        self.firstDetails = firstDetails
        self.secondDetails = secondDetails
        self.hasConflictingDetails = hasConflictingDetails
    }
}

/// The exact bounded data supplied to the model and used to identify a verdict. The policy version covers
/// instruction and formatting changes; no volatile lesson timestamp enters the cache key.
public struct JudgmentInput: Equatable, Sendable {
    public static let policyVersion = 2
    public static let maxLessonLength = 256

    public struct Correction: Codable, Equatable, Sendable {
        public let titleA: String
        public let titleB: String
        public let signalsA: String
        public let signalsB: String
        public let decision: Lesson.Decision
    }

    public let first: AdjudicationEvent
    public let second: AdjudicationEvent
    public let lessons: [Correction]
    public let startOffsetMinutes: Int
    public let endOffsetMinutes: Int
    public let overlapMinutes: Int
    public let firstDetails: String
    public let secondDetails: String
    public let hasConflictingDetails: Bool

    public init(_ request: AdjudicationRequest) {
        first = request.first.promptBounded()
        second = request.second.promptBounded()
        lessons = request.lessons.prefix(LessonBook.promptLimit).map {
            Correction(titleA: PromptText.bounded(String($0.titleA.prefix(Self.maxLessonLength)), quotedLimit: 50),
                       titleB: PromptText.bounded(String($0.titleB.prefix(Self.maxLessonLength)), quotedLimit: 50),
                       signalsA: PromptText.bounded(String($0.signalsA.prefix(Self.maxLessonLength)), quotedLimit: 30),
                       signalsB: PromptText.bounded(String($0.signalsB.prefix(Self.maxLessonLength)), quotedLimit: 30), decision: $0.decision)
        }
        startOffsetMinutes = request.startOffsetMinutes
        endOffsetMinutes = request.endOffsetMinutes
        overlapMinutes = request.overlapMinutes
        firstDetails = request.firstDetails
        secondDetails = request.secondDetails
        hasConflictingDetails = request.hasConflictingDetails
    }

    public func cacheKey(engineID: String, policyVersion: Int = Self.policyVersion) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        encoder.dateEncodingStrategy = .millisecondsSince1970
        func encoded<T: Encodable>(_ value: T) -> String {
            String(decoding: (try? encoder.encode(value)) ?? Data(), as: UTF8.self)
        }
        let sides = [encoded(Side(event: first, details: firstDetails)),
                     encoded(Side(event: second, details: secondDetails))].sorted()
        let corrections = lessons.map(encoded).sorted()
        let parts = [String(policyVersion), engineID] + sides + corrections +
            [String(abs(startOffsetMinutes)), String(abs(endOffsetMinutes)), String(overlapMinutes),
             String(hasConflictingDetails)]
        return "j\(policyVersion)-" + Fingerprint.fnv1a(parts.map { "\($0.utf8.count):\($0)" }.joined())
    }

    private struct Side: Encodable {
        let event: AdjudicationEvent
        let details: String
    }
}

public struct AdjudicationVerdict: Equatable, Sendable {
    public enum Answer: String, Codable, Sendable { case same, different, unsure }
    public var requestID: String
    public var answer: Answer

    public init(requestID: String, answer: Answer) {
        self.requestID = requestID
        self.answer = answer
    }
}

/// A platform's on-device model. Implementations live outside Core.
public protocol DuplicateAdjudicator: Sendable {
    var availability: AdjudicatorAvailability { get }
    /// Verdicts for the requests it could judge; omit a request on failure so it is retried later.
    func judge(_ requests: [AdjudicationRequest]) async -> [AdjudicationVerdict]
}

/// Model verdicts, kept so each pair is judged once. Entries expire by age (`retention`) and the
/// cap, never by event end: the fetch window includes events that already ended today, and dropping
/// their verdicts would make the pair pending (and judged) again on every pass.
public struct VerdictCache: Codable, Equatable, Sendable {
    public static let retention: TimeInterval = 7 * 24 * 60 * 60
    public static let maxEntries = 1000

    public struct Entry: Codable, Equatable, Sendable {
        public var answer: AdjudicationVerdict.Answer
        public var engine: EngineInfo
        public var decidedAt: Date
        public var end: Date
    }

    public private(set) var entries: [String: Entry] = [:]

    public init() {}

    public func entry(for requestID: String) -> Entry? { entries[requestID] }

    public mutating func store(_ verdict: AdjudicationVerdict, engine: EngineInfo, end: Date, now: Date) {
        entries[verdict.requestID] = Entry(answer: verdict.answer, engine: engine, decidedAt: now, end: end)
    }

    public mutating func discardIncompatible() {
        entries = entries.filter { $0.key.hasPrefix("j\(JudgmentInput.policyVersion)-") }
    }

    /// True if anything was dropped.
    @discardableResult
    public mutating func prune(now: Date) -> Bool {
        let before = entries.count
        entries = entries.filter { now.timeIntervalSince($0.value.decidedAt) <= Self.retention }
        if entries.count > Self.maxEntries {
            let oldestFirst = entries.sorted { ($0.value.decidedAt, $0.key) < ($1.value.decidedAt, $1.key) }
            for (key, _) in oldestFirst.prefix(entries.count - Self.maxEntries) { entries[key] = nil }
        }
        return entries.count != before
    }
}

enum Fingerprint {
    /// 64-bit FNV-1a as hex. Deterministic across launches and platforms (unlike `Hasher`).
    static func fnv1a(_ text: String) -> String {
        var hash: UInt64 = 0xcbf29ce484222325
        for byte in text.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x100000001b3
        }
        return String(hash, radix: 16)
    }
}
