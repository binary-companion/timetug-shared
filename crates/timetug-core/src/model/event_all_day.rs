use calendar_core::all_day::{AllDay, CalendarDate};

use crate::model::event::TimeTugCalendarEvent;

impl TimeTugCalendarEvent {
    /// The first covered date and the exclusive end date, read in the event's own zone (never the viewer's), so an
    /// all-day event stays on the dates it was created for. `None` for timed events.
    /// A same-date range is treated as one day.
    pub fn all_day_dates(&self) -> Option<(CalendarDate, CalendarDate)> {
        if !self.is_all_day() {
            return None;
        }
        let zone = self.time_zone();
        let (first, end_exclusive) = AllDay::dates(self.start(), self.end(), zone);
        if end_exclusive <= first {
            return Some((first, first.adding_days(1)));
        }
        Some((first, end_exclusive))
    }

    /// True when this is an all-day event whose dates include `date`; always false for timed events.
    pub fn covers(&self, date: &CalendarDate) -> bool {
        match self.all_day_dates() {
            Some((first, end_exclusive)) => first <= *date && *date < end_exclusive,
            None => false,
        }
    }
}
