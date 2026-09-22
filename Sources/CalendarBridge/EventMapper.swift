import CalendarCore
import Foundation
import TimeTugCore

/// Maps the connector library's model to TimeTug's. The library event is carried as is (times, all-day form and
/// attendees are used natively); only the source id and Core's own state are added.
public struct EventMapper: Sendable {
    public init() {}

    public func calendarInfo(_ d: CalendarDescriptor, sourceID: String) -> CalendarInfo {
        CalendarInfo(sourceID: sourceID, calendarID: d.id, title: d.title, accountName: d.accountName, colorHex: d.colorHex, kind: d.kind)
    }

    /// nil for cancelled events. Titles are never rewritten: each connector chooses its own placeholder.
    public func event(_ e: CalendarCore.CalendarEvent, sourceID: String) -> TimeTugCalendarEvent? {
        e.status == .cancelled ? nil : TimeTugCalendarEvent(event: e, sourceID: sourceID)
    }
}
