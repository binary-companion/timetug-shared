import Foundation
import TimeTugKitGenerated

// The convenience initialisers of CalendarCore 1.x (`Model.swift`, `SourceTypes.swift`): the generated memberwise ones
// need every field, these carry the 1.x defaults and labels (`eventID:`, `calendarID:`, `sourceID:`).
//
// Where the 1.x labels equal the generated ones (`CalendarPermissions`, `SourceCapabilities`, `Attendee`, `ConferenceInfo`) the
// initialiser has a last unlabelled parameter `_: Void = ()`. A declaration with the same signature in this module would
// shadow the generated one (and call itself); with the extra defaulted parameter a call that names every field still picks
// the generated initialiser, a call that leaves some out picks this one, and this one can delegate to the generated one.

extension CalendarEvent {
    public init(
        eventID: String, uid: String? = nil, uidScope: UIDScope? = nil, calendarID: String, title: String,
        notes: String? = nil, location: String? = nil, start: Date, end: Date,
        timeZone: TimeZone = TimeZone(identifier: "UTC")!, isAllDay: Bool = false, status: EventStatus = .confirmed,
        availability: Availability? = nil, visibility: Visibility? = nil, kind: EventKind? = nil,
        series: SeriesInfo? = nil, attendees: [Attendee] = [], organizer: Attendee? = nil,
        conferences: [ConferenceInfo] = [], reminders: [Reminder]? = nil, url: URL? = nil, version: String? = nil,
        lastModified: Date? = nil, created: Date? = nil, participation: Participation? = nil, sourceID: String? = nil
    ) {
        self.init(
            eventId: eventID, uid: uid, uidScope: uidScope, calendarId: calendarID, title: title, notes: notes,
            location: location, start: start, end: end, timeZone: timeZone, isAllDay: isAllDay, status: status,
            availability: availability, visibility: visibility, kind: kind, series: series, attendees: attendees,
            organizer: organizer, conferences: conferences, reminders: reminders, url: url, version: version,
            lastModified: lastModified, created: created, participation: participation, sourceId: sourceID)
    }
}

extension CalendarDescriptor {
    /// `colorHex` is normalised to `#RRGGBB` uppercase (nil when invalid), as in 1.x.
    public init(
        id: String, title: String, service: CalendarService, colorHex: String? = nil,
        permissions: CalendarPermissions = CalendarPermissions(), isDefault: Bool? = nil, timeZone: TimeZone? = nil,
        accountName: String? = nil, kind: CalendarKind = .standard, defaultReminders: [Reminder]? = nil,
        provider: CalendarProvider? = nil, supportedAvailabilities: Set<Availability>? = nil
    ) {
        self.init(
            id: id, title: title, colorHex: Self.normalizedHex(colorHex), permissions: permissions, isDefault: isDefault,
            timeZone: timeZone, accountName: accountName, kind: kind, defaultReminders: defaultReminders,
            service: service, provider: provider, supportedAvailabilities: supportedAvailabilities)
    }

    /// "#RGB", "#RRGGBB" or "RRGGBB" (any case) to "#RRGGBB" uppercase; nil for anything else.
    public static func normalizedHex(_ raw: String?) -> String? {
        guard var s = raw?.trimmingCharacters(in: .whitespacesAndNewlines) else { return nil }
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 3 || s.count == 6, s.allSatisfy(\.isASCII), s.allSatisfy(\.isHexDigit) else { return nil }
        s = s.uppercased()
        if s.count == 3 { s = s.map { "\($0)\($0)" }.joined() }
        return "#" + s
    }
}

extension CalendarPermissions {
    public init(
        canViewDetails: Bool = true, canEdit: Bool = false, canShare: Bool? = nil, canViewPrivate: Bool? = nil, _: Void = ()
    ) {
        self.init(canViewDetails: canViewDetails, canEdit: canEdit, canShare: canShare, canViewPrivate: canViewPrivate)
    }
}

extension SourceCapabilities {
    public init(
        canWrite: Bool = false, canEditAttendees: Bool = false, canRespondToInvite: Bool = false,
        providedFields: Set<ProvidedField> = [], syncKind: SyncKind = .none, supportsPush: Bool = false,
        writableFields: Set<EventField> = [], controlsNotifications: Bool = false,
        recurrenceScopes: Set<RecurrenceScope> = [], _: Void = ()
    ) {
        self.init(
            canWrite: canWrite, canEditAttendees: canEditAttendees, canRespondToInvite: canRespondToInvite,
            providedFields: providedFields, syncKind: syncKind, supportsPush: supportsPush, writableFields: writableFields,
            controlsNotifications: controlsNotifications, recurrenceScopes: recurrenceScopes)
    }
}

extension Reminder {
    /// The 1.x spelling: the alert type is `type` (the generated field is `kind`).
    public init(
        trigger: ReminderTrigger, type: ReminderType = .display, repeatCount: Int = 0, repeatInterval: TimeInterval? = nil,
        isCalendarDefault: Bool? = nil
    ) {
        self.init(
            trigger: trigger, kind: type, repeatCount: Int64(repeatCount), repeatInterval: repeatInterval,
            isCalendarDefault: isCalendarDefault)
    }

    public var type: ReminderType { get { kind } set { kind = newValue } }

    /// A relative-to-start reminder of `minutes` before the start.
    public static func before(minutes: Int, type: ReminderType = .display, isCalendarDefault: Bool? = nil) -> Reminder {
        Reminder(trigger: .relative(offset: -(Double(minutes) * 60), to: .start), type: type, isCalendarDefault: isCalendarDefault)
    }
}

extension Attendee {
    /// As in 1.x the address is trimmed and lowercased, and an empty one becomes `nil`.
    public init(
        name: String? = nil, email: String? = nil, role: AttendeeRole = .required, response: ResponseStatus = .needsAction,
        isSelf: Bool = false, isOrganizer: Bool = false, _: Void = ()
    ) {
        let trimmed = email?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        self.init(
            name: name, email: (trimmed?.isEmpty ?? true) ? nil : trimmed, role: role, response: response, isSelf: isSelf,
            isOrganizer: isOrganizer)
    }
}

extension ConferenceInfo {
    /// `origin` defaults to `.structured`.
    public init(url: URL, provider: ConferenceProvider, origin: ConferenceOrigin = .structured, _: Void = ()) {
        self.init(url: url, provider: provider, origin: origin)
    }
}

extension WeekdayOccurrence {
    /// `WeekdayOccurrence(.tuesday, ordinal: 2)`: the second Tuesday; -1 is the last.
    public init(_ weekday: Weekday, ordinal: Int? = nil) {
        self.init(weekday: weekday, ordinal: ordinal.map { Int64($0) })
    }
}
