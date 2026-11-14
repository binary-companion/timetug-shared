import Foundation
import TimeTugKitGenerated

// The Swift names of the 1.x libraries that UniFFI cannot spell the same way (docs/bindings.md, "Names that differ").

public typealias UIDScope = UidScope
public typealias ConnectionID = String

extension RecurrenceRule {
    public typealias End = RuleEnd
}

extension DayAgenda {
    public typealias Item = DayAgendaItem
}

extension DayAgendaItem {
    public typealias State = DayAgendaItemState
}

extension EngineDiagnostic {
    public typealias Level = EngineDiagnosticLevel
}

// UniFFI turns acronyms into words (`eventID` becomes `eventId`, `joinURL` becomes `joinUrl`) and no rename can restore
// them. The old spellings come back as computed properties on the types the Mac app reads and writes.

extension CalendarEvent {
    public var eventID: String { get { eventId } set { eventId = newValue } }
    public var calendarID: String { get { calendarId } set { calendarId = newValue } }
    public var sourceID: String? { get { sourceId } set { sourceId = newValue } }
}

extension TimeTugCalendarEvent {
    public var sourceID: String { get { sourceId } set { sourceId = newValue } }
}

extension CalendarDescriptor {
    public var timeZoneIdentifier: String? { timeZone?.identifier }
}

extension CalendarInfo {
    public var sourceID: String { get { sourceId } set { sourceId = newValue } }
    public var calendarID: String { get { calendarId } set { calendarId = newValue } }
}

extension EventRef {
    public var calendarID: String { get { calendarId } set { calendarId = newValue } }
    public var eventID: String { get { eventId } set { eventId = newValue } }
    public var seriesID: String? { get { seriesId } set { seriesId = newValue } }
}

extension Connection {
    public var kindID: String { get { kindId } set { kindId = newValue } }
    public var connectionID: String { get { connectionId } set { connectionId = newValue } }
}

extension WidgetEvent {
    public var joinURL: URL? { get { joinUrl } set { joinUrl = newValue } }
}

extension TakeoverRequest {
    public var joinURL: URL? { get { joinUrl } set { joinUrl = newValue } }
}

extension ConferenceInfo {
    public var originIsEventURL: Bool { origin == .eventUrl }
}
