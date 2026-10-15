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
    /// What "add account" offers, in display order (CalDAV last); system-permission kinds are not listed.
    public var availableAccountKinds: [AccountKindInfo]

    public static let empty = EngineState(
        revision: 0, agenda: .empty, events: [], calendars: [], statuses: [:], notices: [:], sourceNames: [:],
        candidates: [:], accounts: [], accountStatus: .empty, settings: EngineSettings(),
        inferenceStatus: .disabled, ledger: TakeoverLedger(), nextTakeover: nil, takeoverVisible: false,
        availableAccountKinds: [])
}

/// One choice for "add account". A kind the platform supports but this build cannot use (for example Google
/// without an OAuth client) is listed with `isConfigured` false so the host can explain why it is unavailable.
public struct AccountKindInfo: Sendable, Equatable {
    public enum Authorization: Sendable, Equatable { case oauth, password, system }
    public var id: String
    public var displayName: String
    public var authorization: Authorization
    public var isConfigured: Bool

    public init(id: String, displayName: String, authorization: Authorization, isConfigured: Bool) {
        self.id = id
        self.displayName = displayName
        self.authorization = authorization
        self.isConfigured = isConfigured
    }
}
