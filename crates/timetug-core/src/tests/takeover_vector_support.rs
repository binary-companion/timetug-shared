//! Reads the event and settings forms of `vectors/takeover/*.json` (the exporter's `EventSpec`, documented in
//! `Sources/VectorExporter/EventSpec.swift` and `Takeover.swift`) and applies the ledger operations of a case.

use calendar_core::model::{
    Attendee, AttendeeRole, CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider,
    Participation, ResponseStatus, UidScope,
};
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde_json::Value;

use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::{MergeProvenance, MergedMember};
use crate::model::takeover_settings::TakeoverSettings;
use crate::takeover::guard::{Decision, TakeoverGuard};
use crate::takeover::ledger::TakeoverLedger;

/// An instant from Unix milliseconds (the vector format, conventions section 9).
pub(super) fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap()
}

pub(super) fn ts_of(value: &Value) -> Timestamp {
    ts(value.as_i64().unwrap())
}

fn attendee(j: &Value) -> Attendee {
    Attendee::new(
        j["name"].as_str().map(str::to_string),
        j["email"].as_str(),
        j["role"]
            .as_str()
            .and_then(AttendeeRole::from_raw_value)
            .unwrap_or(AttendeeRole::Required),
        j["response"]
            .as_str()
            .and_then(ResponseStatus::from_raw_value)
            .unwrap_or(ResponseStatus::NeedsAction),
        j["isSelf"].as_bool().unwrap_or(false),
        false,
    )
}

/// The exporter's `EventSpec.event(_:)`.
pub(super) fn event_from_spec(j: &Value) -> TimeTugCalendarEvent {
    let zone = Zone::named(j["timeZone"].as_str().unwrap_or("UTC")).unwrap();
    let mut base = CalendarEvent::new(
        j["eventID"].as_str().unwrap(),
        j["calendarID"].as_str().unwrap(),
        j["title"].as_str().unwrap(),
        ts_of(&j["start"]),
        ts_of(&j["end"]),
    );
    base.uid = j["uid"].as_str().map(str::to_string);
    base.uid_scope = j["uidScope"].as_str().and_then(UidScope::from_raw_value);
    base.notes = j["notes"].as_str().map(str::to_string);
    base.location = j["location"].as_str().map(str::to_string);
    base.time_zone = zone;
    base.is_all_day = j["isAllDay"].as_bool().unwrap_or(false);
    base.attendees = j["attendees"]
        .as_array()
        .map(|a| a.iter().map(attendee).collect())
        .unwrap_or_default();
    base.organizer = j.get("organizer").filter(|o| o.is_object()).map(attendee);
    base.conferences = j["conferences"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|c| {
                    ConferenceInfo::new(
                        UrlString::parse(c["url"].as_str().unwrap()).unwrap(),
                        ConferenceProvider::from_raw_value(c["provider"].as_str().unwrap())
                            .unwrap(),
                        c["origin"]
                            .as_str()
                            .map(|o| ConferenceOrigin::from_raw_value(o).unwrap())
                            .unwrap_or(ConferenceOrigin::Structured),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    base.url = j["url"].as_str().and_then(UrlString::parse);
    base.participation = j["myResponse"]
        .as_str()
        .and_then(ResponseStatus::from_raw_value)
        .map(Participation::Invited);
    let mut event = TimeTugCalendarEvent::new(base, j["sourceID"].as_str().unwrap());
    event.calendar_service = j["calendarService"].as_str().map(str::to_string);
    event.calendar_provider = j["calendarProvider"].as_str().map(str::to_string);
    if let Some(count) = j["otherAttendeeCount"].as_i64() {
        event.other_attendee_count = count;
    }
    if let Some(status) = j.get("responseStatus") {
        event.response_status = status.as_str().and_then(ResponseStatus::from_raw_value);
    }
    if let Some(keys) = j["additionalCalendarKeys"].as_array() {
        event.additional_calendar_keys = keys
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
    }
    if let Some(shown) = j["displayStart"].as_i64() {
        event.display_start = Some(ts(shown));
    }
    event.merged_members = j["mergedMembers"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|m| {
                    MergedMember::new(
                        m["title"].as_str().unwrap(),
                        m["calendarKey"].as_str().unwrap(),
                        m["contentKey"].as_str().unwrap(),
                        m["details"].as_str().unwrap(),
                        ts_of(&m["start"]),
                        ts_of(&m["end"]),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    event.merge_provenance = match j.get("mergeProvenance") {
        Some(Value::String(name)) if name == "rule" => Some(MergeProvenance::Rule),
        Some(Value::String(_)) => Some(MergeProvenance::UserConfirmed),
        Some(Value::Object(object)) => {
            let inner = &object["inference"];
            Some(MergeProvenance::Inference {
                engine_id: inner["engineID"].as_str().unwrap().to_string(),
                engine_name: inner["engineName"].as_str().unwrap().to_string(),
            })
        }
        _ => None,
    };
    event
}

/// The exporter's `EventSpec.settings(_:)`.
pub(super) fn settings_from(j: &Value) -> TakeoverSettings {
    let mut s = TakeoverSettings::default();
    s.lead_time = j["leadTime"].as_f64().unwrap_or(s.lead_time);
    let keys = |name: &str| {
        j[name]
            .as_array()
            .map(|a| a.iter().map(|k| k.as_str().unwrap().to_string()).collect())
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

/// What an operation answers (the `returned` field of a ledger step).
pub(super) fn returned_json(value: Returned) -> Value {
    match value {
        Returned::Null => Value::Null,
        Returned::Count(n) => Value::from(n as i64),
        Returned::Changed(b) => Value::Bool(b),
        Returned::Decision(text) => Value::String(text),
    }
}

pub(super) enum Returned {
    Null,
    Count(usize),
    Changed(bool),
    Decision(String),
}

/// The exporter's `TakeoverAreas.Runner`: a pool of events and one ledger that starts empty.
pub(super) struct Runner {
    pub events: Vec<TimeTugCalendarEvent>,
    pub ledger: TakeoverLedger,
}

impl Runner {
    pub fn new(specs: &Value) -> Runner {
        Runner {
            events: specs
                .as_array()
                .unwrap()
                .iter()
                .map(event_from_spec)
                .collect(),
            ledger: TakeoverLedger::new(),
        }
    }

    pub fn apply(&mut self, op: &Value) -> Returned {
        let kind = op["op"].as_str().unwrap();
        let index = |key: &str| op[key].as_u64().unwrap() as usize;
        match kind {
            "markFired" => {
                let event = self.events[index("event")].clone();
                self.ledger.mark_fired(&event, ts_of(&op["now"]));
            }
            "snooze" => {
                let event = self.events[index("event")].clone();
                self.ledger
                    .snooze(&event, op["duration"].as_f64().unwrap(), ts_of(&op["now"]));
            }
            "acknowledgeInProgress" => {
                let events: Vec<TimeTugCalendarEvent> = op["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| self.events[i.as_u64().unwrap() as usize].clone())
                    .collect();
                return Returned::Count(self.ledger.acknowledge_in_progress(
                    &events,
                    ts_of(&op["now"]),
                    op["grace"].as_f64().unwrap(),
                ));
            }
            "prune" => return Returned::Changed(self.ledger.prune(ts_of(&op["now"]))),
            "guard" => {
                let current: Vec<TimeTugCalendarEvent> = op["current"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| self.events[i.as_u64().unwrap() as usize].clone())
                    .collect();
                let decision = TakeoverGuard::evaluate(
                    &self.events[index("event")],
                    &current,
                    &settings_from(&op["settings"]),
                    &self.ledger,
                    ts_of(&op["now"]),
                    op["overlayVisible"].as_bool().unwrap(),
                );
                return Returned::Decision(match decision {
                    Decision::Present => "present".to_string(),
                    Decision::Suppress(reason) => format!("suppress:{}", reason.as_str()),
                });
            }
            "reload" => {
                let text = serde_json::to_string(&self.ledger).unwrap();
                self.ledger = serde_json::from_str(&text).unwrap();
            }
            "load" => {
                self.ledger = serde_json::from_value(op["json"].clone()).unwrap();
            }
            other => panic!("unknown op {other}"),
        }
        Returned::Null
    }
}

/// Compares two JSON values; numbers may differ by `tolerance`. Swift's `Date` is one `Double` of seconds since 2001
/// and the Rust `Timestamp` is whole nanoseconds, so a fractional reference-date number can differ in its last digit
/// (a double near 8e8 has a spacing of about 1.2e-7).
pub(super) fn json_close(expected: &Value, actual: &Value, tolerance: f64) -> bool {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() <= tolerance
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| json_close(x, y, tolerance))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, x)| b.get(k).is_some_and(|y| json_close(x, y, tolerance)))
        }
        (a, b) => a == b,
    }
}
