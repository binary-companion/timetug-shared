import Foundation

/// Pure functions that turn a `WidgetSnapshot` into what a widget shows at a given instant.
public enum WidgetTimeline {
    public struct NextUp: Equatable, Sendable {
        public let current: WidgetEvent?
        public let upcoming: [WidgetEvent]
        public init(current: WidgetEvent?, upcoming: [WidgetEvent]) {
            self.current = current
            self.upcoming = upcoming
        }
    }

    public struct TodayRow: Equatable, Sendable, Identifiable {
        public let event: WidgetEvent
        public let state: DayAgenda.State
        public var id: String { event.id }
    }

    public static func changeDates(snapshot: WidgetSnapshot, now: Date, calendar: Calendar, limit: Int) -> [Date] {
        var dates = Set<Date>()
        for event in snapshot.events where !event.isAllDay {
            if event.start > now { dates.insert(event.start) }
            if event.end > now { dates.insert(event.end) }
        }
        let dayStart = calendar.startOfDay(for: now)
        for offset in 1..<WidgetSnapshot.horizonDays {
            if let midnight = calendar.date(byAdding: .day, value: offset, to: dayStart), midnight > now {
                dates.insert(midnight)
            }
        }
        return Array(dates.sorted().prefix(limit))
    }

    public static func nextUp(snapshot: WidgetSnapshot, now: Date, upcomingLimit: Int) -> NextUp {
        let timed = snapshot.events.filter { !$0.isAllDay }
        let current = timed.filter { $0.start <= now && $0.end > now }.min { $0.start < $1.start }
        let upcoming = timed.filter { $0.start > now }.sorted { ($0.start, $0.title) < ($1.start, $1.title) }
        return NextUp(current: current, upcoming: Array(upcoming.prefix(upcomingLimit)))
    }

    public static func today(snapshot: WidgetSnapshot, now: Date, calendar: Calendar) -> [TodayRow] {
        let dayStart = calendar.startOfDay(for: now)
        let nextDayStart = calendar.date(byAdding: .day, value: 1, to: dayStart)!
        return snapshot.events
            .filter { $0.end > dayStart && $0.start < nextDayStart }
            .map { event in
                let state: DayAgenda.State = event.end <= now ? .past : (event.start <= now ? .current : .upcoming)
                return TodayRow(event: event, state: state)
            }
    }
}
