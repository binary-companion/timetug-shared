import Foundation

/// Decides whether an event may take over the screen. Pure and platform-neutral.
public enum TakeoverPolicy {
    public static func qualifies(_ event: CalendarEvent, settings: TakeoverSettings) -> Bool {
        guard settings.takeoverCalendarKeys.contains(event.calendarKey) else { return false }
        if event.isAllDay { return false }
        if settings.skipDeclinedEvents, event.responseStatus == .declined { return false }
        if settings.skipSoloEvents, event.otherAttendeeCount == 0 { return false }
        if settings.requireConferenceLink, event.conferenceURL == nil { return false }
        return true
    }
}
