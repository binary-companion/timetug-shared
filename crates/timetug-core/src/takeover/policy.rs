use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use calendar_core::model::ResponseStatus;

/// Decides whether an event may take over the screen. Pure and platform-neutral.
pub enum TakeoverPolicy {}

impl TakeoverPolicy {
    /// True when `event` may take over the screen under `settings`: takeover is enabled, a calendar the event is on is
    /// opted in and not hidden, and the event is timed, not declined, has other attendees when
    /// `require_other_attendees` is set, and has a conference link when `require_conference_link` is set.
    pub fn qualifies(event: &TimeTugCalendarEvent, settings: &TakeoverSettings) -> bool {
        if !settings.enabled {
            return false;
        }
        // A calendar that is not shown never tugs: some copy of the event must be on a calendar that is both opted in and shown.
        if !event.all_calendar_keys().iter().any(|key| {
            settings.takeover_calendar_keys.contains(key)
                && !settings.hidden_calendar_keys.contains(key)
        }) {
            return false;
        }
        // All-day events never take over, regardless of settings.
        if event.is_all_day() {
            return false;
        }
        // Declined events never take over, even though the agenda still lists them.
        if event.response_status == Some(ResponseStatus::Declined) {
            return false;
        }
        if settings.require_other_attendees && event.other_attendee_count == 0 {
            return false;
        }
        if settings.require_conference_link && event.conference_url().is_none() {
            return false;
        }
        true
    }
}
