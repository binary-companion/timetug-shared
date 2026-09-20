import Foundation

public struct ScheduledTakeover: Equatable, Sendable {
    public let event: TimeTugCalendarEvent
    public let fireAt: Date
}

public enum Scheduler {
    /// The next takeover to arm, or nil. `fireAt` is never earlier than `now` (late fires are
    /// immediate while the meeting is still in progress).
    public static func next(
        events: [TimeTugCalendarEvent], settings: TakeoverSettings, ledger: TakeoverLedger, now: Date
    ) -> ScheduledTakeover? {
        var best: ScheduledTakeover?
        for event in events where event.end > now && TakeoverPolicy.qualifies(event, settings: settings) {
            let fireAt: Date
            switch ledger.entry(for: event) {
            case .fired: continue
            case .snoozed(let until): fireAt = max(until, now)
            case nil: fireAt = max(event.start.addingTimeInterval(-settings.leadTime), now)
            }
            guard fireAt < event.end else { continue }
            if best == nil || fireAt < best!.fireAt {
                best = ScheduledTakeover(event: event, fireAt: fireAt)
            }
        }
        return best
    }
}
