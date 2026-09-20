import Foundation

/// Decides whether an event may take over the screen. Pure and platform-neutral.
public enum TakeoverPolicy {
    public static func qualifies(_ event: TimeTugCalendarEvent, settings: TakeoverSettings) -> Bool {
        if settings.disabled { return false }
        guard !settings.takeoverCalendarKeys.isDisjoint(with: event.allCalendarKeys) else { return false }
        // All-day events never take over, regardless of settings.
        if event.isAllDay { return false }
        if settings.skipDeclinedEvents, event.responseStatus == .declined { return false }
        if settings.skipSoloEvents, event.otherAttendeeCount == 0 { return false }
        if settings.requireConferenceLink, event.conferenceURL == nil { return false }
        return true
    }
}
