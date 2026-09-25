import Foundation

public enum MergeReason: String, Sendable { case exactMatch, externalUID, conferenceLink, sharedAttendee, sameLocation, sameTitle }
public enum SeparateReason: String, Sendable {
    case sameCalendar, allDay, outsideTimeGate, conflictingLocation, conflictingConference, conflictingAttendees
}
public enum PairDecision: Equatable, Sendable {
    case merge(MergeReason)
    case separate(SeparateReason)
    /// Nothing matched and nothing conflicted: the only case a model may be asked about.
    case ambiguous
}

/// Deterministic pair rules. Pure; no model involved.
public enum DuplicateRules {
    public static let maxStartDifference: TimeInterval = 30 * 60
    public static let maxEndDifference: TimeInterval = 60 * 60
    private static let minContainedLocationLength = 6

    /// Different calendars, timed, and inside the time gate.
    public static func isCandidate(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> Bool {
        a.calendarKey != b.calendarKey && !a.isAllDay && !b.isAllDay && withinTimeGate(a, b)
    }

    /// What the user may merge by hand: two timed events whose ranges actually overlap, on any calendar (the same
    /// one included) and with none of the automatic gate's limits. Back-to-back events (one ends as the other
    /// starts) do not overlap. The context menu and the resolver's use of a user's "same" lesson both use this,
    /// so the menu only offers pairs the resolver will honor.
    public static func isManualCandidate(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> Bool {
        !a.isAllDay && !b.isAllDay && a.start < b.end && b.start < a.end
    }

    /// Overlapping, starts within 30 min, ends within 60 min (end times are uncertain and may include travel).
    public static func withinTimeGate(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> Bool {
        a.start < b.end && b.start < a.end
            && abs(a.start.timeIntervalSince(b.start)) <= maxStartDifference
            && abs(a.end.timeIntervalSince(b.end)) <= maxEndDifference
    }

    public static func decide(_ a: TimeTugCalendarEvent, _ b: TimeTugCalendarEvent) -> PairDecision {
        if a.contentKey == b.contentKey { return .merge(.exactMatch) }
        if a.calendarKey == b.calendarKey { return .separate(.sameCalendar) }
        if a.isAllDay || b.isAllDay { return .separate(.allDay) }
        if !withinTimeGate(a, b) { return .separate(.outsideTimeGate) }

        if let uid = a.externalUID, uid == b.externalUID { return .merge(.externalUID) }
        let confA = conferenceIdentities(a), confB = conferenceIdentities(b)
        if !confA.isDisjoint(with: confB) { return .merge(.conferenceLink) }
        // Vetoes run before the weaker merge signals: a shared person or place merges two events
        // only when nothing conflicts (user decision 2026-09-19).
        let location = locationRelation(a.location, b.location)
        if location == .conflict { return .separate(.conflictingLocation) }
        // Both have links and none is shared (the shared case merged above).
        if !confA.isEmpty, !confB.isEmpty { return .separate(.conflictingConference) }
        let emailsA = emails(a), emailsB = emails(b)
        if !emailsA.isDisjoint(with: emailsB) { return .merge(.sharedAttendee) }
        if location == .same { return .merge(.sameLocation) }
        if !emailsA.isEmpty, !emailsB.isEmpty { return .separate(.conflictingAttendees) }
        // The same title inside the time gate is one event copied onto two calendars (the ends often differ);
        // it comes after every veto so two same-named meetings with different rooms or people stay apart.
        let title = normalize(a.title)
        if !title.isEmpty, title == normalize(b.title) { return .merge(.sameTitle) }
        return .ambiguous
    }

    /// Lowercased, accent-folded, punctuation collapsed to single spaces.
    public static func normalize(_ text: String) -> String {
        let folded = text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
        return String(folded.map { $0.isLetter || $0.isNumber ? $0 : " " })
            .split(separator: " ").joined(separator: " ")
    }

    public static func detailScore(_ event: TimeTugCalendarEvent) -> Int { detailParts(event).count }

    /// "bare" or a "+"-joined list of location, conference, attendees, notes.
    public static func detailSummary(_ event: TimeTugCalendarEvent) -> String {
        let parts = detailParts(event)
        return parts.isEmpty ? "bare" : parts.joined(separator: "+")
    }

    private static func detailParts(_ event: TimeTugCalendarEvent) -> [String] {
        var parts: [String] = []
        if normalizedLocation(event.location) != nil { parts.append("location") }
        if !conferenceIdentities(event).isEmpty { parts.append("conference") }
        if event.otherAttendeeCount > 0 || !event.attendees.isEmpty { parts.append("attendees") }
        if !(event.notes?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true) { parts.append("notes") }
        return parts
    }

    /// The identity of each conference link the event has (see `ConferenceInfo.identity`). The event's own
    /// generic `url` (origin `.eventURL`) never identifies a meeting; a provider-supplied link counts even for a
    /// provider not on the allowlist (Chime, say).
    static func conferenceIdentities(_ event: TimeTugCalendarEvent) -> Set<String> {
        Set(event.conferences.filter { $0.origin != .eventURL }.map(\.identity))
    }

    static func emails(_ event: TimeTugCalendarEvent) -> Set<String> {
        Set((event.attendees.map(\.email) + [event.organizerEmail]).compactMap { $0 })
    }

    private enum LocationRelation { case unknown, same, conflict }

    private static func locationRelation(_ a: String?, _ b: String?) -> LocationRelation {
        guard let a = normalizedLocation(a), let b = normalizedLocation(b) else { return .unknown }
        if a == b { return .same }
        let (short, long) = a.count <= b.count ? (a, b) : (b, a)
        if short.count >= minContainedLocationLength, " \(long) ".contains(" \(short) ") { return .same }
        return .conflict
    }

    /// nil for empty text and for URLs (a link in the location field is a conference, not a place).
    static func normalizedLocation(_ raw: String?) -> String? {
        guard let raw, !raw.contains("://") else { return nil }
        let normalized = normalize(raw)
        return normalized.isEmpty ? nil : normalized
    }
}

extension MergedMember {
    public init(_ event: TimeTugCalendarEvent) {
        self.init(title: event.title, calendarKey: event.calendarKey, contentKey: event.contentKey,
                  details: DuplicateRules.detailSummary(event), start: event.start, end: event.end)
    }
}

extension TimeTugCalendarEvent {
    /// The (title, calendar) copies this event stands for: itself when it was never merged.
    public var participants: [MergedMember] { mergedMembers.isEmpty ? [MergedMember(self)] : mergedMembers }
}
