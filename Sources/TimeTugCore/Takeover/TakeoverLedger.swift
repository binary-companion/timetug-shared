import Foundation

/// Remembers which events already took over (or are snoozed) so a refresh never repeats one.
/// Keyed by `CalendarEvent.id`, which includes the start time, so a rescheduled event is new.
public struct TakeoverLedger: Equatable, Sendable {
    enum Entry: Equatable, Sendable {
        case fired
        case snoozed(until: Date)
    }

    private var entries: [String: Entry] = [:]
    private var endDates: [String: Date] = [:]

    public init() {}

    public mutating func markFired(_ event: CalendarEvent) {
        entries[event.id] = .fired
        endDates[event.id] = event.end
    }

    /// Re-arms the event `duration` from now, never past the meeting's end.
    public mutating func snooze(_ event: CalendarEvent, for duration: TimeInterval, now: Date) {
        entries[event.id] = .snoozed(until: min(now.addingTimeInterval(duration), event.end))
        endDates[event.id] = event.end
    }

    /// Drops entries for events that have ended (not wholesale, so midnight rollover is safe).
    public mutating func prune(now: Date) {
        for (id, end) in endDates where end <= now {
            entries[id] = nil
            endDates[id] = nil
        }
    }

    func entry(for event: CalendarEvent) -> Entry? { entries[event.id] }
}
