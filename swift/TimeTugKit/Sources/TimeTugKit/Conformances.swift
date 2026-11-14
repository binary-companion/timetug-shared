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
