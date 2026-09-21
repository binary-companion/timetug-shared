import Foundation

/// One original copy folded into a merged event.
public struct MergedMember: Hashable, Sendable {
    public var title: String
    public var calendarKey: String
    public var contentKey: String
    /// Which details the original copy had, e.g. "bare" or "location+notes" (see `DuplicateRules.detailSummary`).
    public var details: String
    /// The original copy's own time range (a merged event shows only one of them).
    public var start: Date
    public var end: Date

    public init(title: String, calendarKey: String, contentKey: String, details: String, start: Date, end: Date) {
        self.title = title
        self.calendarKey = calendarKey
        self.contentKey = contentKey
        self.details = details
        self.start = start
        self.end = end
    }
}

/// How a merged event came to be merged. Core carries data only; the app words the badge.
public enum MergeProvenance: Hashable, Sendable {
    case rule
    case inference(engineID: String, engineName: String)
    case userConfirmed
}
