import CalendarCore
import Foundation

/// Today's events as plain data for any front end. Presentation (greying, text) is the app's job.
public struct DayAgenda: Equatable, Sendable {
    public enum State: Equatable, Sendable { case past, current, upcoming }

    public struct Item: Equatable, Sendable, Identifiable {
        public let event: TimeTugCalendarEvent
        public let state: State
        public var id: String { event.id }
    }

    public let items: [Item]
    /// First timed (non-all-day) event that has not started yet.
    public let next: TimeTugCalendarEvent?

    public static let empty = DayAgenda(items: [], next: nil)

    public static func make(
        events: [TimeTugCalendarEvent], settings: TakeoverSettings, now: Date, calendar: Calendar
    ) -> DayAgenda {
        let dayStart = calendar.startOfDay(for: now)
        let nextDayStart = calendar.date(byAdding: .day, value: 1, to: dayStart)!

        let today = AllDay.date(of: dayStart, in: calendar.timeZone)

        let items = events
            // A merged meeting is hidden only if every calendar it appears on is hidden.
            .filter { !$0.allCalendarKeys.isSubset(of: settings.hiddenCalendarKeys) }
            .filter { !(settings.skipAllDayEvents && $0.isAllDay) }
            .filter { event in
                if let dates = event.allDayDates {
                    return dates.first <= today && today < dates.endExclusive
                }
                if event.end > dayStart && event.start < nextDayStart { return true }
                // After-midnight events show only once inside their lead-time period.
                return !event.isAllDay
                    && event.start >= nextDayStart
                    && now >= event.start.addingTimeInterval(-settings.leadTime)
            }
            .sorted { ($0.start, $0.title) < ($1.start, $1.title) }
            .map { event -> Item in
                let state: State
                if let dates = event.allDayDates {
                    state = dates.endExclusive <= today ? .past : (dates.first <= today ? .current : .upcoming)
                } else {
                    state = event.end <= now ? .past : (event.start <= now ? .current : .upcoming)
                }
                return Item(event: event, state: state)
            }

        let next = items.first { !$0.event.isAllDay && $0.event.start > now }?.event
        return DayAgenda(items: items, next: next)
    }
}
