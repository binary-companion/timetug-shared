use calendar_core::time::add_seconds;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;

use crate::model::event::TimeTugCalendarEvent;

/// Everything a front end needs to render a takeover. Plain data: no display strings.
#[derive(Clone, Debug, PartialEq)]
pub struct TakeoverRequest {
    pub event: TimeTugCalendarEvent,
    pub has_started: bool,
    pub join_url: Option<UrlString>,
    /// Snooze durations (seconds) that still end before the meeting does.
    pub snooze_options: Vec<f64>,
}

impl TakeoverRequest {
    pub const SNOOZE_CHOICES: [f64; 3] = [60.0, 300.0, 600.0];

    /// Swift `make(for:now:)`.
    pub fn make(event: &TimeTugCalendarEvent, now: Timestamp) -> TakeoverRequest {
        TakeoverRequest {
            event: event.clone(),
            has_started: now >= event.start(),
            join_url: event.conference_url().cloned(),
            snooze_options: Self::SNOOZE_CHOICES
                .iter()
                .copied()
                .filter(|choice| add_seconds(now, *choice) < event.end())
                .collect(),
        }
    }
}
