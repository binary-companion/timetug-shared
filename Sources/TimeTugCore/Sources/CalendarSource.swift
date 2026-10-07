import CalendarCore
import Foundation

public enum SourceStatus: Equatable, Sendable {
    case ok
    case needsPermission
    case authExpired
    case failing(String)
}

/// Sources throw these to report a status the user can act on.
public enum SourceError: Error, Sendable {
    case needsPermission
    case authExpired
}

/// A source notification retains its origin even when the connector cannot narrow the fetch to a calendar.
public enum SourceChange: Equatable, Sendable {
    case calendarsChanged(sourceID: String)
    case eventsChanged(sourceID: String, calendarIDs: Set<String>?)
    case sourceFailed(sourceID: String)

    public var sourceID: String {
        switch self {
        case .calendarsChanged(let id), .eventsChanged(let id, _), .sourceFailed(let id): id
        }
    }
}

/// A calendar backend. Returns only Core's normalized model; source-specific types never leak.
public protocol CalendarSource: Sendable {
    var id: String { get }
    var displayName: String { get }
    func calendars() async throws -> [CalendarInfo]
    func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent]
    /// Yields whenever the source's data may have changed.
    func changes() -> AsyncStream<SourceChange>
    /// What the source noticed in the data it last loaded (for example a warning icon). Optional: the default is none.
    func notices() async -> [CalendarCore.SourceNotice]
}

extension CalendarSource {
    public func notices() async -> [CalendarCore.SourceNotice] { [] }
}
