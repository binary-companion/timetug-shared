//! Builders for the vector files of the agenda and widget areas: the `EventSpec` event shape and the settings
//! object of the Swift `VectorExporter` (`EventSpec.swift`), and the `WidgetEvent` shape of `Agenda.swift`.

use calendar_core::model::{CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde_json::{Value, json};

use crate::agenda::day_agenda::State;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use crate::widget::snapshot::WidgetEvent;

use super::support::zone;

pub(super) fn ms(value: &Value) -> Timestamp {
    Timestamp::from_millisecond(value.as_i64().unwrap()).unwrap()
}

pub(super) fn to_ms(t: Timestamp) -> i64 {
    t.as_millisecond()
}

/// `EventSpec.event` for the members the agenda and widget vectors use.
pub(super) fn event_from_spec(j: &Value) -> TimeTugCalendarEvent {
    let mut base = CalendarEvent::new(
        j["eventID"].as_str().unwrap(),
        j["calendarID"].as_str().unwrap(),
        j["title"].as_str().unwrap(),
        ms(&j["start"]),
        ms(&j["end"]),
    );
    base.time_zone = zone(j["timeZone"].as_str().unwrap_or("UTC"));
    base.is_all_day = j["isAllDay"].as_bool().unwrap_or(false);
    base.conferences = j["conferences"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|c| {
                    ConferenceInfo::new(
                        UrlString::parse(c["url"].as_str().unwrap()).unwrap(),
                        ConferenceProvider::from_raw_value(c["provider"].as_str().unwrap())
                            .unwrap(),
                        ConferenceOrigin::from_raw_value(c["origin"].as_str().unwrap()).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut event = TimeTugCalendarEvent::new(base, j["sourceID"].as_str().unwrap());
    if let Some(keys) = j["additionalCalendarKeys"].as_array() {
        event.additional_calendar_keys = keys
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
    }
    if let Some(shown) = j.get("displayStart").filter(|v| v.is_number()) {
        event.display_start = Some(ms(shown));
    }
    event
}

/// `EventSpec.settings`.
pub(super) fn settings_from(j: &Value) -> TakeoverSettings {
    let mut s = TakeoverSettings::default();
    if let Some(lead) = j["leadTime"].as_f64() {
        s.lead_time = lead;
    }
    let keys = |name: &str| -> std::collections::BTreeSet<String> {
        j[name]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|k| k.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    s.takeover_calendar_keys = keys("takeoverCalendarKeys");
    s.hidden_calendar_keys = keys("hiddenCalendarKeys");
    s.require_conference_link = j["requireConferenceLink"].as_bool().unwrap_or(false);
    s.require_other_attendees = j["requireOtherAttendees"].as_bool().unwrap_or(false);
    s.skip_all_day_events = j["skipAllDayEvents"].as_bool().unwrap_or(true);
    s.enabled = j["enabled"].as_bool().unwrap_or(true);
    s
}

pub(super) fn state_name(state: State) -> &'static str {
    match state {
        State::Past => "past",
        State::Current => "current",
        State::Upcoming => "upcoming",
    }
}

pub(super) fn state_from(name: &str) -> State {
    match name {
        "past" => State::Past,
        "current" => State::Current,
        _ => State::Upcoming,
    }
}

pub(super) fn widget_event_json(e: &WidgetEvent) -> Value {
    let mut j = json!({
        "id": e.id, "title": e.title, "start": to_ms(e.start), "end": to_ms(e.end), "isAllDay": e.is_all_day
    });
    if let Some(hex) = &e.color_hex {
        j["colorHex"] = json!(hex);
    }
    if let Some(url) = &e.join_url {
        j["joinURL"] = json!(url.as_str());
    }
    j
}

pub(super) fn widget_event(j: &Value) -> WidgetEvent {
    WidgetEvent::new(
        j["id"].as_str().unwrap(),
        j["title"].as_str().unwrap(),
        ms(&j["start"]),
        ms(&j["end"]),
        j["isAllDay"].as_bool().unwrap(),
        j["colorHex"].as_str().map(str::to_string),
        j["joinURL"].as_str().and_then(UrlString::parse),
    )
}

/// Reports every failing case at once, as the vector convention requires.
pub(super) fn assert_no_failures(area: &str, total: usize, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{area}: {} of {total} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
