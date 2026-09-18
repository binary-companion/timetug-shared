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

/// A calendar backend. Returns only Core's normalized model; source-specific types never leak.
public protocol CalendarSource: Sendable {
    var id: String { get }
    var displayName: String { get }
    func calendars() async throws -> [CalendarInfo]
    func events(in interval: DateInterval) async throws -> [CalendarEvent]
    /// Yields whenever the source's data may have changed.
    func changes() -> AsyncStream<Void>
}
