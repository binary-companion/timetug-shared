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
public struct AdjudicationEvent: Equatable, Sendable {
    public static let maxNotesLength = 500

    public var title: String
    public var start: Date
    public var end: Date
    public var location: String?
    public var notes: String?
    public var attendeeNames: [String]
    public var calendarTitle: String?
    public var accountName: String?

    public init(_ event: CalendarEvent, calendar: CalendarInfo?) {
        title = event.title
        start = event.start
        end = event.end
        location = event.location
        notes = event.notes.map { String($0.prefix(Self.maxNotesLength)) }
        attendeeNames = event.attendees.compactMap(\.name).filter { !$0.isEmpty }
        calendarTitle = calendar?.title
        accountName = calendar?.accountName
    }
}

public struct AdjudicationRequest: Equatable, Sendable {
    /// Fingerprint of both events' relevant content; the cache key.
    public var id: String
    /// The more detailed event (ties: the earlier one); the other is `second`.
    public var first: AdjudicationEvent
    public var second: AdjudicationEvent
    public var lessons: [Lesson]

    public init(id: String, first: AdjudicationEvent, second: AdjudicationEvent, lessons: [Lesson]) {
        self.id = id
        self.first = first
        self.second = second
        self.lessons = lessons
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

/// Model verdicts, kept so each pair is judged once. Ended, old and surplus entries are pruned.
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

    /// True if anything was dropped.
    @discardableResult
    public mutating func prune(now: Date) -> Bool {
        let before = entries.count
        entries = entries.filter { $0.value.end > now && now.timeIntervalSince($0.value.decidedAt) <= Self.retention }
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
