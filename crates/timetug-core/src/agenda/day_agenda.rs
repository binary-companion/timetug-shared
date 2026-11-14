use calendar_core::all_day::AllDay;
use calendar_core::civil::{self, Unit};
use calendar_core::time::{Zone, add_seconds};
use jiff::Timestamp;

use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;

/// Today's events as plain data for any front end. Presentation (greying, text) is the app's job.
#[derive(Clone, Debug, PartialEq)]
pub struct DayAgenda {
    /// The day's events, sorted by start then title.
    pub items: Vec<Item>,
    /// First timed (non-all-day) event that has not started yet.
    pub next: Option<TimeTugCalendarEvent>,
}

/// `DayAgenda.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    /// The event has ended.
    Past,
    /// The event is under way.
    Current,
    /// The event has not started yet.
    Upcoming,
}

/// `DayAgenda.Item`.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The event (a merged event stands for all its calendars).
    pub event: TimeTugCalendarEvent,
    /// Where the event stands relative to now.
    pub state: State,
}

impl Item {
    /// `Identifiable.id`: the event's id.
    pub fn id(&self) -> String {
        self.event.id()
    }
}

impl DayAgenda {
    /// `DayAgenda.empty`.
    pub fn empty() -> DayAgenda {
        DayAgenda {
            items: Vec::new(),
            next: None,
        }
    }

    /// Builds today's agenda from `events` at `now` in `zone`. Events on calendars hidden in `settings` and, when
    /// `skip_all_day_events` is set, all-day events are left out; an event starting after midnight appears only once
    /// its lead time has begun.
    pub fn make(
        events: &[TimeTugCalendarEvent],
        settings: &TakeoverSettings,
        now: Timestamp,
        zone: &Zone,
    ) -> DayAgenda {
        let day_start = civil::start_of_day(now, zone);
        let next_day_start = next_day(day_start, zone);

        let today = AllDay::date_of(day_start, zone);

        let mut included: Vec<&TimeTugCalendarEvent> = events
            .iter()
            // A merged meeting is hidden only if every calendar it appears on is hidden.
            .filter(|e| {
                !e.all_calendar_keys()
                    .is_subset(&settings.hidden_calendar_keys)
            })
            .filter(|e| !(settings.skip_all_day_events && e.is_all_day()))
            .filter(|event| {
                if let Some((first, end_exclusive)) = event.all_day_dates() {
                    return first <= today && today < end_exclusive;
                }
                if event.end() > day_start && event.start() < next_day_start {
                    return true;
                }
                // After-midnight events show only once inside their lead-time period.
                !event.is_all_day()
                    && event.start() >= next_day_start
                    && now >= add_seconds(event.start(), -settings.lead_time)
            })
            .collect();
        // Stable, like Swift's `sorted`; titles compare by unicode scalar value.
        included.sort_by(|a, b| (a.start(), a.title()).cmp(&(b.start(), b.title())));
        let items: Vec<Item> = included
            .into_iter()
            .map(|event| {
                let state = if let Some((first, end_exclusive)) = event.all_day_dates() {
                    if end_exclusive <= today {
                        State::Past
                    } else if first <= today {
                        State::Current
                    } else {
                        State::Upcoming
                    }
                } else if event.end() <= now {
                    State::Past
                } else if event.start() <= now {
                    State::Current
                } else {
                    State::Upcoming
                };
                Item {
                    event: event.clone(),
                    state,
                }
            })
            .collect();

        let next = items
            .iter()
            .find(|i| !i.event.is_all_day() && i.event.start() > now)
            .map(|i| i.event.clone());
        DayAgenda { items, next }
    }
}

/// `calendar.date(byAdding: .day, value: 1, to: dayStart)!`. Swift traps outside the range of a `Date`; this falls
/// back to 24 hours later instead.
pub(crate) fn next_day(day_start: Timestamp, zone: &Zone) -> Timestamp {
    civil::add(Unit::Day, 1, day_start, zone).unwrap_or_else(|| add_seconds(day_start, 86_400.0))
}
