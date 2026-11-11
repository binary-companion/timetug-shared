use calendar_core::time::add_seconds;
use jiff::Timestamp;

use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use crate::takeover::ledger::{Entry, TakeoverLedger};
use crate::takeover::policy::TakeoverPolicy;

#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledTakeover {
    pub event: TimeTugCalendarEvent,
    pub fire_at: Timestamp,
}

pub enum Scheduler {}

impl Scheduler {
    /// The next takeover to arm, or `None`. `fire_at` is never earlier than `now` (late fires are
    /// immediate while the meeting is still in progress).
    pub fn next(
        events: &[TimeTugCalendarEvent],
        settings: &TakeoverSettings,
        ledger: &TakeoverLedger,
        now: Timestamp,
    ) -> Option<ScheduledTakeover> {
        let mut best: Option<ScheduledTakeover> = None;
        for event in events
            .iter()
            .filter(|e| e.end() > now && TakeoverPolicy::qualifies(e, settings))
        {
            let fire_at = match ledger.entry_for(event) {
                Some(Entry::Fired) => continue,
                Some(Entry::Snoozed { until }) => until.max(now),
                None => add_seconds(event.start(), -settings.lead_time).max(now),
            };
            if fire_at >= event.end() {
                continue;
            }
            if best.as_ref().is_none_or(|b| fire_at < b.fire_at) {
                best = Some(ScheduledTakeover {
                    event: event.clone(),
                    fire_at,
                });
            }
        }
        best
    }
}
