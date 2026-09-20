import Foundation

/// Everything a front end needs to render a takeover. Plain data: no display strings.
public struct TakeoverRequest: Equatable, Sendable {
    public static let snoozeChoices: [TimeInterval] = [60, 300, 600]

    public let event: TimeTugCalendarEvent
    public let hasStarted: Bool
    public let joinURL: URL?
    /// Snooze durations (seconds) that still end before the meeting does.
    public let snoozeOptions: [TimeInterval]

    public static func make(for event: TimeTugCalendarEvent, now: Date) -> TakeoverRequest {
        TakeoverRequest(
            event: event,
            hasStarted: now >= event.start,
            joinURL: event.conferenceURL,
            snoozeOptions: snoozeChoices.filter { now.addingTimeInterval($0) < event.end }
        )
    }
}
