use jiff::Timestamp;

use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use crate::takeover::ledger::TakeoverLedger;
use crate::takeover::policy::TakeoverPolicy;

/// The last check before a takeover is shown. Timers capture an event when armed; by the time one
/// fires the meeting may be gone, edited, declined, opted out or already handled.
pub enum TakeoverGuard {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Present,
    Suppress(Reason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    NotInSnapshot,
    NoLongerQualifies,
    AlreadyFired,
    Ended,
    OverlayVisible,
}

impl Reason {
    /// The Swift raw value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Reason::NotInSnapshot => "notInSnapshot",
            Reason::NoLongerQualifies => "noLongerQualifies",
            Reason::AlreadyFired => "alreadyFired",
            Reason::Ended => "ended",
            Reason::OverlayVisible => "overlayVisible",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Reason> {
        [
            Reason::NotInSnapshot,
            Reason::NoLongerQualifies,
            Reason::AlreadyFired,
            Reason::Ended,
            Reason::OverlayVisible,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == raw)
    }
}

impl TakeoverGuard {
    /// `Decision::Suppress(Reason::OverlayVisible)` must not be marked fired by the caller: the event stays pending
    /// and is picked up when the overlay closes.
    pub fn evaluate(
        event: &TimeTugCalendarEvent,
        current_events: &[TimeTugCalendarEvent],
        settings: &TakeoverSettings,
        ledger: &TakeoverLedger,
        now: Timestamp,
        overlay_visible: bool,
    ) -> Decision {
        if overlay_visible {
            return Decision::Suppress(Reason::OverlayVisible);
        }
        let Some(current) = current_events.iter().find(|e| e.is_same_meeting(event)) else {
            return Decision::Suppress(Reason::NotInSnapshot);
        };
        if event.end() <= now {
            return Decision::Suppress(Reason::Ended);
        }
        if !TakeoverPolicy::qualifies(current, settings) {
            return Decision::Suppress(Reason::NoLongerQualifies);
        }
        if ledger.has_fired(current) {
            return Decision::Suppress(Reason::AlreadyFired);
        }
        Decision::Present
    }
}
