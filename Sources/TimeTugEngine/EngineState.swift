import CalendarCore
import Foundation
import TimeTugCore

/// An immutable-by-convention snapshot of everything a front end renders. `revision` increases on every publish.
public struct EngineState: Sendable {
    public var revision: UInt64
    public var agenda: DayAgenda
    public var events: [TimeTugCalendarEvent]
    public var calendars: [CalendarInfo]
    public var statuses: [String: SourceStatus]
    public var notices: [String: [CalendarCore.SourceNotice]]
    public var sourceNames: [String: String]
    public var candidates: [String: [TimeTugCalendarEvent]]
    public var accounts: [Connection]
    public var accountStatus: AccountManager.Status
    public var settings: EngineSettings
    public var inferenceStatus: InferenceStatus
    public var ledger: TakeoverLedger
    public var nextTakeover: ScheduledTakeover?
    public var takeoverVisible: Bool

    public static let empty = EngineState(
        revision: 0, agenda: .empty, events: [], calendars: [], statuses: [:], notices: [:], sourceNames: [:],
        candidates: [:], accounts: [], accountStatus: .empty, settings: EngineSettings(),
        inferenceStatus: .disabled, ledger: TakeoverLedger(), nextTakeover: nil, takeoverVisible: false)
}
