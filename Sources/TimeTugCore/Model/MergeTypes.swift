import Foundation

/// A meeting invitee other than the calendar owner. Emails are normalized (lowercased, trimmed).
public struct Attendee: Hashable, Sendable {
    public var name: String?
    public private(set) var email: String?

    public init(name: String? = nil, email: String? = nil) {
        self.name = name
        self.email = Self.normalizedEmail(email)
    }

    public static func normalizedEmail(_ raw: String?) -> String? {
        guard let s = raw?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(), !s.isEmpty else { return nil }
        return s
    }

    /// The address in a "mailto:" URL string (as EventKit participants expose it); nil for any other URL.
    public static func email(fromMailto urlString: String?) -> String? {
        guard let urlString, urlString.lowercased().hasPrefix("mailto:") else { return nil }
        let rest = String(urlString.dropFirst("mailto:".count))
        return normalizedEmail(rest.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first.map(String.init))
    }
}

/// One original copy folded into a merged event.
public struct MergedMember: Hashable, Sendable {
    public var title: String
    public var calendarKey: String
    public var contentKey: String
    /// Which details the original copy had, e.g. "bare" or "location+notes" (see `DuplicateRules.detailSummary`).
    public var details: String

    public init(title: String, calendarKey: String, contentKey: String, details: String) {
        self.title = title
        self.calendarKey = calendarKey
        self.contentKey = contentKey
        self.details = details
    }
}

/// How a merged event came to be merged. Core carries data only; the app words the badge.
public enum MergeProvenance: Hashable, Sendable {
    case rule
    case inference(engineID: String, engineName: String)
    case userConfirmed
}
