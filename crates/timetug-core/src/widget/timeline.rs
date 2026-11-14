use std::collections::BTreeSet;

use calendar_core::civil::{self, Unit};
use calendar_core::time::Zone;
use jiff::Timestamp;

use crate::agenda::day_agenda::{State, next_day};
use crate::widget::snapshot::{WidgetEvent, WidgetSnapshot};

/// Pure functions that turn a `WidgetSnapshot` into what a widget shows at a given instant.
pub enum WidgetTimeline {}

/// `WidgetTimeline.NextUp`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextUp {
    /// The timed event under way now, if any (the earliest started when several overlap).
    pub current: Option<WidgetEvent>,
    /// Timed events that have not started, by start then title, limited to the requested count.
    pub upcoming: Vec<WidgetEvent>,
}

impl NextUp {
    /// Creates the value from its parts.
    pub fn new(current: Option<WidgetEvent>, upcoming: Vec<WidgetEvent>) -> NextUp {
        NextUp { current, upcoming }
    }
}

/// `WidgetTimeline.TodayRow`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodayRow {
    /// The snapshot event shown on this row.
    pub event: WidgetEvent,
    /// Whether the event is past, current or upcoming at the time asked.
    pub state: State,
}

impl TodayRow {
    /// Creates the row from its parts.
    pub fn new(event: WidgetEvent, state: State) -> TodayRow {
        TodayRow { event, state }
    }

    /// `Identifiable.id`: the event's id.
    pub fn id(&self) -> String {
        self.event.id.clone()
    }
}

/// Swift's `prefix(n)` with `n` an `Int` (Swift traps on a negative count; this takes none).
fn count(n: i64) -> usize {
    usize::try_from(n.max(0)).unwrap_or(usize::MAX)
}

impl WidgetTimeline {
    /// The instants after `now` at which what a widget shows changes: starts and ends of timed events and the coming
    /// midnights inside the horizon, ascending, at most `limit`.
    pub fn change_dates(
        snapshot: &WidgetSnapshot,
        now: Timestamp,
        zone: &Zone,
        limit: i64,
    ) -> Vec<Timestamp> {
        let mut dates: BTreeSet<Timestamp> = BTreeSet::new();
        for event in snapshot.events.iter().filter(|e| !e.is_all_day) {
            if event.start > now {
                dates.insert(event.start);
            }
            if event.end > now {
                dates.insert(event.end);
            }
        }
        let day_start = civil::start_of_day(now, zone);
        for offset in 1..WidgetSnapshot::HORIZON_DAYS {
            if let Some(midnight) = civil::add(Unit::Day, offset, day_start, zone)
                && midnight > now
            {
                dates.insert(midnight);
            }
        }
        dates.into_iter().take(count(limit)).collect()
    }

    /// The timed event under way at `now` and up to `upcoming_limit` timed events still to start.
    pub fn next_up(snapshot: &WidgetSnapshot, now: Timestamp, upcoming_limit: i64) -> NextUp {
        let timed: Vec<&WidgetEvent> = snapshot.events.iter().filter(|e| !e.is_all_day).collect();
        // `min(by:)` keeps the first of equal elements, as Iterator::min_by_key does.
        let current = timed
            .iter()
            .filter(|e| e.start <= now && e.end > now)
            .min_by_key(|e| e.start)
            .map(|e| (*e).clone());
        let mut upcoming: Vec<&WidgetEvent> = timed.into_iter().filter(|e| e.start > now).collect();
        upcoming.sort_by(|a, b| (a.start, &a.title).cmp(&(b.start, &b.title)));
        NextUp::new(
            current,
            upcoming
                .into_iter()
                .take(count(upcoming_limit))
                .cloned()
                .collect(),
        )
    }

    /// One row for each snapshot event that overlaps the day of `now` in `zone`, in snapshot order.
    pub fn today(snapshot: &WidgetSnapshot, now: Timestamp, zone: &Zone) -> Vec<TodayRow> {
        let day_start = civil::start_of_day(now, zone);
        let next_day_start = next_day(day_start, zone);
        snapshot
            .events
            .iter()
            .filter(|e| e.end > day_start && e.start < next_day_start)
            .map(|event| {
                let state = if event.end <= now {
                    State::Past
                } else if event.start <= now {
                    State::Current
                } else {
                    State::Upcoming
                };
                TodayRow::new(event.clone(), state)
            })
            .collect()
    }

    /// The rows to show when only `capacity` fit. Over capacity, finished meetings are dropped first so the rest of
    /// the day stays visible; if everything is finished, the last `capacity` rows are kept so the day is not shown empty.
    pub fn visible_today(rows: &[TodayRow], capacity: i64) -> Vec<TodayRow> {
        let capacity = count(capacity);
        if rows.len() <= capacity {
            return rows.to_vec();
        }
        let remaining: Vec<TodayRow> = rows
            .iter()
            .filter(|r| r.state != State::Past)
            .cloned()
            .collect();
        if remaining.is_empty() {
            rows[rows.len() - capacity..].to_vec()
        } else {
            remaining
        }
    }
}
