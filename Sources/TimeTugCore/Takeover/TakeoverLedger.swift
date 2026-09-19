import Foundation

/// Remembers which events already took over (or are snoozed) so a refresh, or a relaunch, never
/// repeats one. Every mutation is recorded under `CalendarEvent.id` (includes the start time,
/// so a rescheduled event is new) and every key in `allContentKeys` (survives a changed source id,
/// so a merge or split never re-fires a meeting).
/// Codable so the app can persist it; `prune` keeps the persisted form bounded.
public struct TakeoverLedger: Equatable, Sendable, Codable {
    /// Entries older than this are dropped even if the event has not "ended" (bogus end dates).
    public static let retention: TimeInterval = 7 * 24 * 60 * 60
    /// Last-resort cap. It limits stored keys to `2 * maxEntries`: an event holds an id key plus one
    /// content key per merged member (1 + N keys), so this is roughly per event. Oldest go first.
    public static let maxEntries = 2000

    enum Entry: Equatable, Sendable, Codable {
        case fired
        case snoozed(until: Date)
    }

    struct Record: Equatable, Sendable, Codable {
        var entry: Entry
        var end: Date
        var recordedAt: Date
    }

    /// Decodes one record, yielding nil (instead of failing the whole file) when it is malformed
    /// or lacks `recordedAt`; such a record is dropped rather than trusted.
    private struct LenientRecord: Decodable {
        let record: Record?
        init(from decoder: Decoder) throws { record = try? Record(from: decoder) }
    }

    private enum CodingKeys: String, CodingKey { case records }

    private var records: [String: Record] = [:]

    public init() {}

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        records = try container.decode([String: LenientRecord].self, forKey: .records).compactMapValues(\.record)
    }

    public mutating func markFired(_ event: CalendarEvent, now: Date) {
        record(.fired, for: event, now: now)
    }

    /// Re-arms the event `duration` from now, never past the meeting's end.
    public mutating func snooze(_ event: CalendarEvent, for duration: TimeInterval, now: Date) {
        record(.snoozed(until: min(now.addingTimeInterval(duration), event.end)), for: event, now: now)
    }

    /// Marks every un-ledgered event that started before `now - grace` and has not ended as fired.
    /// Used once per launch so a meeting already underway never takes over. Returns the count.
    @discardableResult
    public mutating func acknowledgeInProgress(
        events: [CalendarEvent], now: Date, grace: TimeInterval
    ) -> Int {
        var count = 0
        for event in events where entry(for: event) == nil
            && event.start < now.addingTimeInterval(-grace) && event.end > now {
            markFired(event, now: now)
            count += 1
        }
        return count
    }

    /// Drops entries whose event has ended or that were recorded more than `retention` ago (not
    /// wholesale, so midnight rollover is safe), then enforces `maxEntries`. True if anything changed.
    @discardableResult
    public mutating func prune(now: Date) -> Bool {
        let before = records.count
        let cutoff = now.addingTimeInterval(-Self.retention)
        records = records.filter { $0.value.end > now && $0.value.recordedAt > cutoff }

        let limit = Self.maxEntries * 2
        if records.count > limit {
            let oldestFirst = records.sorted { ($0.value.recordedAt, $0.key) < ($1.value.recordedAt, $1.key) }
            for (key, _) in oldestFirst.prefix(records.count - limit) { records[key] = nil }
        }
        return records.count != before
    }

    /// True only for the `.fired` state; a snoozed event is pending.
    public func hasFired(_ event: CalendarEvent) -> Bool { entry(for: event) == .fired }

    /// True while the event is snoozed (pending, not fired).
    public func isSnoozed(_ event: CalendarEvent) -> Bool {
        if case .snoozed = entry(for: event) { return true }
        return false
    }

    /// Number of stored keys (per remembered event: its id key plus one content key per merged member). For diagnostics.
    public var keyCount: Int { records.count }

    func entry(for event: CalendarEvent) -> Entry? {
        let found = ([event.id] + event.allContentKeys.sorted()).compactMap { records[$0]?.entry }
        return found.first(where: { $0 == .fired }) ?? found.first
    }

    private mutating func record(_ entry: Entry, for event: CalendarEvent, now: Date) {
        let record = Record(entry: entry, end: event.end, recordedAt: now)
        records[event.id] = record
        for key in event.allContentKeys { records[key] = record }
    }
}
