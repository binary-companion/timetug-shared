import CalendarCore
import Foundation
import TimeTugCore

/// Adapts a connector-library source to Core's `CalendarSource`, so `CalendarStore` never sees library types.
public final class ConnectedSource: TimeTugCore.CalendarSource, Sendable {
    private let source: any CalendarCore.CalendarSource
    private let mapper: EventMapper

    public init(_ source: any CalendarCore.CalendarSource, mapper: EventMapper = EventMapper()) {
        self.source = source
        self.mapper = mapper
    }

    public var id: String { source.id }
    public var displayName: String { source.displayName }

    public func calendars() async throws -> [CalendarInfo] {
        do { return try await source.calendars().map { mapper.calendarInfo($0, sourceID: id) } }
        catch { throw Self.translate(error) }
    }

    public func events(in interval: DateInterval) async throws -> [TimeTugCalendarEvent] {
        do { return try await source.events(in: interval).compactMap { mapper.event($0, sourceID: id) } }
        catch { throw Self.translate(error) }
    }

    /// Yields once for every library change. `.sourceFailed` yields too, so the next refresh surfaces the status.
    /// Cancelling the consumer cancels the inner task, which ends the library stream (and its polling).
    public func changes() -> AsyncStream<Void> {
        let source = source
        return AsyncStream { continuation in
            let task = Task {
                for await _ in source.changes() { continuation.yield() }
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    private static func translate(_ error: Error) -> Error {
        switch error as? CalendarCore.SourceError {
        case .needsPermission?: TimeTugCore.SourceError.needsPermission
        case .authExpired?: TimeTugCore.SourceError.authExpired
        default: error
        }
    }
}
