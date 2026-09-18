import Foundation

/// The last check before a takeover is shown. Timers capture an event when armed; by the time one
/// fires the meeting may be gone, edited, declined, opted out or already handled.
public enum TakeoverGuard {
    public enum Decision: Equatable, Sendable {
        case present
        case suppress(Reason)
    }

    public enum Reason: String, Equatable, Sendable {
        case notInSnapshot, noLongerQualifies, alreadyFired, ended, overlayVisible
    }

    /// `.suppress(.overlayVisible)` must not be marked fired by the caller: the event stays pending
    /// and is picked up when the overlay closes.
    public static func evaluate(
        event: CalendarEvent, currentEvents: [CalendarEvent], settings: TakeoverSettings,
        ledger: TakeoverLedger, now: Date, overlayVisible: Bool
    ) -> Decision {
        if overlayVisible { return .suppress(.overlayVisible) }
        guard let current = currentEvents.first(where: {
            $0.id == event.id || $0.contentKey == event.contentKey
        }) else { return .suppress(.notInSnapshot) }
        if event.end <= now { return .suppress(.ended) }
        if !TakeoverPolicy.qualifies(current, settings: settings) { return .suppress(.noLongerQualifies) }
        if ledger.hasFired(current) { return .suppress(.alreadyFired) }
        return .present
    }
}
