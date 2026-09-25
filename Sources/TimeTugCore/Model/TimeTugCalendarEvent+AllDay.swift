import CalendarCore
import Foundation

extension TimeTugCalendarEvent {
    /// The first covered date and the exclusive end date, read in the event's own zone (never the viewer's), so an
    /// all-day event stays on the dates it was created for. nil for timed events.
    /// A same-date range is treated as one day.
    public var allDayDates: (first: CalendarDate, endExclusive: CalendarDate)? {
        guard isAllDay else { return nil }
        let zone = timeZone
        let dates = AllDay.dates(start: start, end: end, in: zone)
        if dates.endExclusive <= dates.first {
            return (first: dates.first, endExclusive: dates.first.adding(days: 1))
        }
        return dates
    }

    public func covers(_ date: CalendarDate) -> Bool {
        guard let dates = allDayDates else { return false }
        return dates.first <= date && date < dates.endExclusive
    }
}
