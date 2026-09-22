import Foundation

/// Decides whether an event may take over the screen. Pure and platform-neutral.
public enum TakeoverPolicy {
    public static func qualifies(_ event: TimeTugCalendarEvent, settings: TakeoverSettings) -> Bool {
        if !settings.enabled { return false }
        // A calendar that is not shown never tugs: some copy of the event must be on a calendar that is both opted in and shown.
        guard event.allCalendarKeys.contains(where: {
            settings.takeoverCalendarKeys.contains($0) && !settings.hiddenCalendarKeys.contains($0)
        }) else { return false }
        // All-day events never take over, regardless of settings.
        if event.isAllDay { return false }
        // Declined events never take over, even though the agenda still lists them.
        if event.responseStatus == .declined { return false }
        if settings.requireOtherAttendees, event.otherAttendeeCount == 0 { return false }
        if settings.requireConferenceLink, event.conferenceURL == nil { return false }
        return true
    }
}
