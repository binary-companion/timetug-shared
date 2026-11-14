//! Port of `Scheduling/UITickPolicy.swift`.

use calendar_core::civil::{self, Unit};
use calendar_core::time::{Zone, add_seconds, seconds_between};
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;

/// Chooses the next UI wake without rebuilding the agenda for every countdown second.
pub enum UITickPolicy {}

/// `UITickPolicy.Wake`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wake {
    /// When the UI should wake next.
    pub date: Timestamp,
    /// True when the day agenda must be rebuilt at this wake, false when republishing the cached agenda is enough.
    pub rebuild_agenda: bool,
}

impl UITickPolicy {
    /// How long before a meeting the "soon" presentation begins (mirrors the Mac menu-bar icon window).
    pub const SOON_WINDOW: f64 = 10.0 * 60.0;

    /// `next(now:countdownVisible:nextEvent:events:leadTime:calendar:)`; the Gregorian `calendar` is its `zone`.
    pub fn next(
        now: Timestamp,
        countdown_visible: bool,
        next_event: Option<&TimeTugCalendarEvent>,
        events: &[TimeTugCalendarEvent],
        lead_time: f64,
        zone: &Zone,
    ) -> Option<Wake> {
        let mut choices: Vec<Wake> = Vec::new();
        let today = civil::start_of_day(now, zone);
        // Swift force-unwraps `date(byAdding:)`; there is no trap here, a day is 24 hours when the calendar has none.
        let midnight = civil::add(Unit::Day, 1, today, zone)
            .unwrap_or_else(|| add_seconds(today, 24.0 * 3600.0));
        choices.push(Wake {
            date: midnight,
            rebuild_agenda: true,
        });
        for event in events {
            for boundary in [event.start(), event.end()] {
                if boundary > now {
                    choices.push(Wake {
                        date: boundary,
                        rebuild_agenda: true,
                    });
                }
            }
            let agenda_lead = add_seconds(event.start(), -lead_time);
            if agenda_lead > now {
                choices.push(Wake {
                    date: agenda_lead,
                    rebuild_agenda: true,
                });
            }
            let soon = add_seconds(event.start(), -Self::SOON_WINDOW);
            if soon > now {
                choices.push(Wake {
                    date: soon,
                    rebuild_agenda: false,
                });
            }
        }
        if let Some(next_event) = next_event
            && countdown_visible
            && next_event.start() > now
        {
            let remaining = seconds_between(now, next_event.start());
            let unit = if remaining >= 60.0 { 60.0 } else { 1.0 };
            let fraction = remaining % unit;
            // `compact` truncates whole seconds, so an exact boundary changes just after now.
            let delay = fraction + 0.02;
            choices.push(Wake {
                date: add_seconds(now, delay),
                rebuild_agenda: false,
            });
        }
        // `min(by:)` keeps the first of equal dates, as Swift's does.
        choices
            .into_iter()
            .reduce(|best, wake| if wake.date < best.date { wake } else { best })
    }
}
