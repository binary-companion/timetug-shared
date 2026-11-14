import Foundation
import TimeTugKitGenerated

// Conformances and computed properties of the Swift model that the generator does not write (taken from Model.swift of
// the 1.x libraries; only those the Mac app uses).

extension CalendarEvent: Identifiable {
    /// Unique across the calendars of one source.
    public var id: String { "\(calendarId)/\(eventId)" }

    /// The series id when this is an instance of a series; nil for a single event and when unknown.
    public var seriesID: String? {
        if case .occurrence(let seriesId, _)? = series { seriesId } else { nil }
    }

    public var originalStart: Date? {
        if case .occurrence(_, let start)? = series { start } else { nil }
    }

    /// The first (most likely) link.
    public var conference: ConferenceInfo? { conferences.first }

    /// The account owner's own response; nil both when unknown and when not invited.
    public var myResponse: ResponseStatus? {
        if case .invited(let response)? = participation { response } else { nil }
    }
}

extension CalendarDate: Comparable {
    public static func < (lhs: CalendarDate, rhs: CalendarDate) -> Bool {
        (lhs.year, lhs.month, lhs.day) < (rhs.year, rhs.month, rhs.day)
    }
}

extension Reminder {
    /// Minutes before the start, when the reminder is relative to the start and not after it.
    public var minutesBefore: Int? {
        guard case .relative(let offset, let anchor) = trigger, anchor == .start, offset <= 0 else { return nil }
        return Int((-offset / 60).rounded())
    }

    public init(minutesBefore: Int) {
        self.init(
            trigger: .relative(offset: -(Double(minutesBefore) * 60), to: .start), kind: .display, repeatCount: 0,
            repeatInterval: nil, isCalendarDefault: nil)
    }
}

extension CalendarPermissions {
    /// A read-only summary of the permissions.
    public var accessRole: AccessRole? {
        if !canViewDetails { return .freeBusyReader }
        if !canEdit { return .reader }
        switch canShare {
        case true?: return .owner
        case false?: return .writer
        case nil: return nil
        }
    }
}

extension CalendarDescriptor {
    public var accessRole: AccessRole? { permissions.accessRole }
}

// What a throwing call of the bindings throws is `FfiError`; its payload mirrors are plain enums (UniFFI renders the cases
// of its own error enums in upper camel case), and these conformances let the old `throw`/`catch` code read the same.
extension SourceError: Error {}
extension WriteError: Error {}

// MARK: - TimeTugCalendarEvent, CalendarInfo, Connection (TimeTugCore `Model/` and CalendarCore `Connection.swift`, 1.x)

extension TimeTugCalendarEvent: Identifiable {
    /// A library event as TimeTug sees it, with what `TimeTugCalendarEvent.init(event:sourceID:)` derives (1.x): the
    /// event's own join links, the number of attendees besides the owner, and the owner's response (`participation`, else
    /// the attendee marked as self).
    public init(event: CalendarEvent, sourceID: String) {
        self.init(
            event: event, sourceId: sourceID, conferences: event.conferences,
            otherAttendeeCount: Int64(event.attendees.filter { !$0.isSelf }.count),
            responseStatus: event.myResponse ?? event.attendees.first(where: \.isSelf)?.response,
            additionalCalendarKeys: [], mergedMembers: [], mergeProvenance: nil, calendarService: nil, calendarProvider: nil,
            displayStart: nil)
    }

    public var title: String { get { event.title } set { event.title = newValue } }
    public var start: Date { get { event.start } set { event.start = newValue } }
    public var end: Date { get { event.end } set { event.end = newValue } }
    /// All-day events use the library's canonical form: midnight of the first day in `timeZone`, `end` exclusive.
    public var isAllDay: Bool { get { event.isAllDay } set { event.isAllDay = newValue } }
    public var timeZone: TimeZone { get { event.timeZone } set { event.timeZone = newValue } }
    public var location: String? { get { event.location } set { event.location = newValue } }
    public var notes: String? { get { event.notes } set { event.notes = newValue } }
    public var url: URL? { get { event.url } set { event.url = newValue } }
    public var calendarID: String { get { event.calendarId } set { event.calendarId = newValue } }

    /// The first join link.
    public var conferenceURL: URL? { conferences.first?.url }

    /// The start of the range to display: `displayStart` when set, else `start`.
    public var shownStart: Date { displayStart ?? start }

    /// Unique per occurrence: recurring events share a source id but differ in start. The engine's `TimeTugCalendarEvent::id`.
    public var id: String { "\(sourceId)/\(event.eventId)/\(Int(start.timeIntervalSince1970))" }

    /// Identity by content (title, start, end): survives a changed source event id, matching the store's duplicate merge.
    public var contentKey: String {
        "\(title.lowercased())|\(Int(start.timeIntervalSince1970))|\(Int(end.timeIntervalSince1970))"
    }

    public var calendarKey: String { CalendarInfo.key(sourceID: sourceId, calendarID: calendarID) }

    /// This event's own calendar plus every calendar its duplicates appear on.
    public var allCalendarKeys: Set<String> { additionalCalendarKeys.union([calendarKey]) }
}

extension CalendarInfo: Identifiable {
    public init(
        sourceID: String, calendarID: String, title: String, accountName: String? = nil, colorHex: String? = nil,
        kind: CalendarKind = .standard, service: CalendarService? = nil, provider: CalendarProvider? = nil
    ) {
        self.init(
            sourceId: sourceID, calendarId: calendarID, title: title, accountName: accountName,
            colorHex: CalendarDescriptor.normalizedHex(colorHex), kind: kind, service: service, provider: provider)
    }

    /// `"<source id>/<calendar id>"`, what `TakeoverSettings` stores.
    public var key: String { Self.key(sourceID: sourceId, calendarID: calendarId) }
    public var id: String { key }

    public static func key(sourceID: String, calendarID: String) -> String { "\(sourceID)/\(calendarID)" }

    public static func normalizedHex(_ raw: String?) -> String? { CalendarDescriptor.normalizedHex(raw) }
}

extension Connection: Identifiable {
    public init(kindID: String, connectionID: ConnectionID, displayName: String, config: [String: String] = [:]) {
        self.init(kindId: kindID, connectionId: connectionID, displayName: displayName, config: config)
    }

    public var id: ConnectionID { connectionId }

    /// The `CalendarSource.id` for this connection. Every consumer must use this helper.
    public var sourceID: String { "\(kindId)-\(connectionId)" }
}

// MARK: - CalendarService and CalendarProvider values (CalendarCore `Model.swift`, 1.x)

extension CalendarService: RawRepresentable {
    public static let eventKit = CalendarService(rawValue: "eventkit")
    public static let google = CalendarService(rawValue: "google")
    public static let microsoft = CalendarService(rawValue: "microsoft")
    public static let calDAV = CalendarService(rawValue: "caldav")
    /// An iCalendar subscription link.
    public static let iCalSubscription = CalendarService(rawValue: "icalsub")
}

extension CalendarProvider: RawRepresentable {
    public static let iCloud = CalendarProvider(rawValue: "icloud")
    public static let google = CalendarProvider(rawValue: "google")
    /// Exchange, Outlook.com, Microsoft 365.
    public static let microsoft = CalendarProvider(rawValue: "microsoft")
    /// On the device.
    public static let local = CalendarProvider(rawValue: "local")
    /// A read-only feed (subscribed or birthday calendar).
    public static let subscription = CalendarProvider(rawValue: "subscription")
    /// A CalDAV server whose host is not known.
    public static let calDAV = CalendarProvider(rawValue: "caldav")
}

// MARK: - Connector diagnostics

extension DiagnosticSink {
    /// Connector events (`record`) were added to the sink in 2.1.0; a sink that only reads engine lines (`log`) keeps
    /// compiling and drops them.
    public func record(event: DiagnosticEvent) {}
}
