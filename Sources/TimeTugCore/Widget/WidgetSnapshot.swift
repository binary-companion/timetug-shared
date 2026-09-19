import Foundation

/// One event as a widget needs it. Times are the ones shown to the user.
public struct WidgetEvent: Codable, Equatable, Sendable, Identifiable {
    public let id: String
    public let title: String
    public let start: Date
    public let end: Date
    public let isAllDay: Bool
    public let colorHex: String?
    public let joinURL: URL?

    public init(id: String, title: String, start: Date, end: Date, isAllDay: Bool = false,
                colorHex: String? = nil, joinURL: URL? = nil) {
        self.id = id
        self.title = title
        self.start = start
        self.end = end
        self.isAllDay = isAllDay
        self.colorHex = colorHex
        self.joinURL = joinURL
    }
}

/// The agenda a front end hands to widgets: today plus the next days, already merged, filtered and sorted.
public struct WidgetSnapshot: Codable, Equatable, Sendable {
    /// Days of events included, counting today.
    public static let horizonDays = 3
    /// A snapshot older than this is treated as missing by widgets.
    public static let maxAge: TimeInterval = 12 * 3600

    public var generatedAt: Date
    public var events: [WidgetEvent]

    public init(generatedAt: Date, events: [WidgetEvent]) {
        self.generatedAt = generatedAt
        self.events = events
    }

    public func isStale(now: Date) -> Bool {
        now.timeIntervalSince(generatedAt) > Self.maxAge
    }

    public static func make(
        events: [CalendarEvent], calendars: [CalendarInfo], settings: TakeoverSettings,
        now: Date, calendar: Calendar
    ) -> WidgetSnapshot {
        let dayStart = calendar.startOfDay(for: now)
        let horizonEnd = calendar.date(byAdding: .day, value: horizonDays, to: dayStart)!
        let colors = Dictionary(calendars.compactMap { info in info.colorHex.map { (info.key, $0) } },
                                uniquingKeysWith: { first, _ in first })
        let included = events
            .filter { !$0.allCalendarKeys.isSubset(of: settings.hiddenCalendarKeys) }
            .filter { !(settings.skipAllDayEvents && $0.isAllDay) }
            .filter { $0.end > dayStart && $0.shownStart < horizonEnd }
            .sorted { ($0.shownStart, $0.title) < ($1.shownStart, $1.title) }
            .map { event in
                WidgetEvent(id: event.id, title: event.title, start: event.shownStart, end: event.end,
                            isAllDay: event.isAllDay, colorHex: colors[event.calendarKey], joinURL: event.conferenceURL)
            }
        return WidgetSnapshot(generatedAt: now, events: included)
    }
}
