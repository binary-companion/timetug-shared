import Foundation
import TimeTugCore

/// A platform or system condition the host reports to the engine.
public enum SystemEvent: Equatable, Sendable {
    case wake, clockChanged, timeZoneChanged, dayChanged, networkReturned, displayChanged
}

/// Everything a front end can ask the engine to do. Events are identified by `TimeTugCalendarEvent.id`.
public enum EngineCommand: Sendable {
    case start
    case refresh
    case addAccount(kindID: String)
    case removeAccount(connectionID: String)
    case reauthorizeAccount(connectionID: String)
    case cancelAuthorization
    case applySettings(@Sendable (inout EngineSettings) -> Void)
    case settingsChangedExternally
    case setCountdownVisible(Bool)
    /// `onScreen` false means the front end fell back to a toast.
    case takeoverPresented(eventID: String, onScreen: Bool)
    case takeoverClosed(eventID: String)
    case join(eventID: String)
    case snooze(eventID: String, seconds: TimeInterval)
    case dismiss(eventID: String)
    /// Settings' "Test tug": a sample takeover. The host supplies the display strings (the engine produces none).
    case testTug(title: String, calendarTitle: String)
    case merge(TimeTugCalendarEvent, TimeTugCalendarEvent)
    case unmerge(TimeTugCalendarEvent)
    case separate(TimeTugCalendarEvent, members: [MergedMember])
    case forgetCorrections
    case system(SystemEvent)
    case stop
}
