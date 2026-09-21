import Foundation

public enum MergeReason: String, Sendable { case exactMatch, externalUID, conferenceLink, sharedAttendee, sameLocation }
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
        let confA = conferenceIdentity(a), confB = conferenceIdentity(b)
        if let confA, confA == confB { return .merge(.conferenceLink) }
        // Vetoes run before the weaker merge signals: a shared person or place merges two events
        // only when nothing conflicts (user decision 2026-09-19).
        let location = locationRelation(a.location, b.location)
        if location == .conflict { return .separate(.conflictingLocation) }
        if let confA, let confB, confA != confB { return .separate(.conflictingConference) }
        let emailsA = emails(a), emailsB = emails(b)
        if !emailsA.isDisjoint(with: emailsB) { return .merge(.sharedAttendee) }
        if location == .same { return .merge(.sameLocation) }
        if !emailsA.isEmpty, !emailsB.isEmpty { return .separate(.conflictingAttendees) }
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
        if conferenceIdentity(event) != nil { parts.append("conference") }
        if event.otherAttendeeCount > 0 || !event.attendees.isEmpty { parts.append("attendees") }
        if !(event.notes?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true) { parts.append("notes") }
        return parts
    }

    /// host + path of the conference link (lowercased), from the structured link or one found in the
    /// location, url or notes. A structured link always counts; a detected one must be a recognised provider (not a generic
    /// `url` field). Webex keeps the meeting id in the `MTID` query item, so that is part of the identity.
    static func conferenceIdentity(_ event: TimeTugCalendarEvent) -> String? {
        // A source-supplied link is trusted as is (unlisted providers like Chime still identify a meeting);
        // a link detected from free text must be a recognised provider (not a generic `url`).
        let url: URL?
        if let structured = event.conferenceURL {
            url = structured
        } else {
            url = ConferenceLinkDetector.detect(location: event.location, url: event.url, notes: event.notes)
                .flatMap { ConferenceLinkDetector.isProvider($0) ? $0 : nil }
        }
        guard let url, let host = url.host?.lowercased() else { return nil }
        var path = url.path.lowercased()
        while path.hasSuffix("/") { path.removeLast() }
        var identity = host + path
        if host == "webex.com" || host.hasSuffix(".webex.com"),
           let meetingID = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?
               .first(where: { $0.name.lowercased() == "mtid" })?.value {
            identity += "?mtid=" + meetingID.lowercased()
        }
        return identity
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
