import Foundation
import TimeTugCore

/// One-shot notifications from the engine to its front end (state itself arrives as `EngineState` snapshots).
public enum EngineEvent: Sendable {
    case takeoverDue(TakeoverRequest, calendarTitle: String?)
    case takeoverWithdrawn(eventID: String)
    case widgetSnapshot(WidgetSnapshot)
    /// The host opens this with its own safe-open policy.
    case joinRequested(URL)
    case diagnostic(EngineDiagnostic)
}
