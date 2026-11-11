//! Replays `vectors/dedup/pair-decisions.json` (640 pairs) and `vectors/dedup/resolutions.json` (310 resolver runs),
//! made by the Swift code at the baseline. The event shape is "EventSpec" of `Sources/VectorExporter/EventSpec.swift`.

use calendar_core::model::{
    Attendee, AttendeeRole, CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider,
    Participation, ResponseStatus, UidScope,
};
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::dedup::adjudication::{AdjudicationVerdict, EngineInfo, VerdictAnswer, VerdictCache};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::resolver::DuplicateResolver;
use crate::dedup::rules::{DuplicateRules, PairDecision};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::{MergeProvenance, MergedMember};

const PAIR_DECISIONS: &str = include_str!("../../../../vectors/dedup/pair-decisions.json");
const RESOLUTIONS: &str = include_str!("../../../../vectors/dedup/resolutions.json");

fn ms(value: &Value) -> Timestamp {
    Timestamp::from_millisecond(value.as_i64().unwrap()).unwrap()
}

fn ms_value(t: Timestamp) -> Value {
    json!(t.as_millisecond())
}

fn text(j: &Value, key: &str) -> Option<String> {
    j.get(key).and_then(Value::as_str).map(str::to_string)
}

fn spec_attendee(j: &Value) -> Attendee {
    Attendee::new(
        text(j, "name"),
        j.get("email").and_then(Value::as_str),
        text(j, "role")
            .and_then(|r| AttendeeRole::from_raw_value(&r))
            .unwrap_or(AttendeeRole::Required),
        text(j, "response")
            .and_then(|r| ResponseStatus::from_raw_value(&r))
            .unwrap_or(ResponseStatus::NeedsAction),
        j.get("isSelf").and_then(Value::as_bool).unwrap_or(false),
        false,
    )
}

/// `EventSpec.event`.
pub(super) fn event_from_spec(j: &Value) -> TimeTugCalendarEvent {
    let zone = Zone::named(j.get("timeZone").and_then(Value::as_str).unwrap_or("UTC")).unwrap();
    let conferences: Vec<ConferenceInfo> = j
        .get("conferences")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|c| {
                    ConferenceInfo::new(
                        UrlString::parse(c["url"].as_str().unwrap()).unwrap(),
                        ConferenceProvider::from_raw_value(c["provider"].as_str().unwrap())
                            .unwrap(),
                        ConferenceOrigin::from_raw_value(
                            c.get("origin")
                                .and_then(Value::as_str)
                                .unwrap_or("structured"),
                        )
                        .unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut base = CalendarEvent::new(
        j["eventID"].as_str().unwrap(),
        j["calendarID"].as_str().unwrap(),
        j["title"].as_str().unwrap(),
        ms(&j["start"]),
        ms(&j["end"]),
    );
    base.uid = text(j, "uid");
    base.uid_scope = text(j, "uidScope").and_then(|s| UidScope::from_raw_value(&s));
    base.notes = text(j, "notes");
    base.location = text(j, "location");
    base.time_zone = zone;
    base.is_all_day = j.get("isAllDay").and_then(Value::as_bool).unwrap_or(false);
    base.attendees = j
        .get("attendees")
        .and_then(Value::as_array)
        .map(|list| list.iter().map(spec_attendee).collect())
        .unwrap_or_default();
    base.organizer = j
        .get("organizer")
        .filter(|o| o.is_object())
        .map(spec_attendee);
    base.conferences = conferences;
    base.url = text(j, "url").and_then(|u| UrlString::parse(&u));
    base.participation = text(j, "myResponse")
        .and_then(|r| ResponseStatus::from_raw_value(&r))
        .map(Participation::Invited);
    let mut event = TimeTugCalendarEvent::new(base, j["sourceID"].as_str().unwrap());
    event.calendar_service = text(j, "calendarService");
    event.calendar_provider = text(j, "calendarProvider");
    if let Some(count) = j.get("otherAttendeeCount").and_then(Value::as_i64) {
        event.other_attendee_count = count;
    }
    if j.get("responseStatus").is_some() {
        event.response_status =
            text(j, "responseStatus").and_then(|s| ResponseStatus::from_raw_value(&s));
    }
    if let Some(keys) = j.get("additionalCalendarKeys").and_then(Value::as_array) {
        event.additional_calendar_keys = keys
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect();
    }
    if let Some(shown) = j.get("displayStart").filter(|v| v.is_number()) {
        event.display_start = Some(ms(shown));
    }
    event.merged_members = j
        .get("mergedMembers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|m| {
                    MergedMember::new(
                        m["title"].as_str().unwrap(),
                        m["calendarKey"].as_str().unwrap(),
                        m["contentKey"].as_str().unwrap(),
                        m["details"].as_str().unwrap(),
                        ms(&m["start"]),
                        ms(&m["end"]),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    event
}

/// `EventSpec.summary`.
fn summary(e: &TimeTugCalendarEvent) -> Value {
    let provenance = match &e.merge_provenance {
        None => Value::Null,
        Some(MergeProvenance::Rule) => json!("rule"),
        Some(MergeProvenance::UserConfirmed) => json!("userConfirmed"),
        Some(MergeProvenance::Inference {
            engine_id,
            engine_name,
        }) => json!({"inference": {"engineID": engine_id, "engineName": engine_name}}),
    };
    json!({
        "id": e.id(), "sourceID": e.source_id, "eventID": e.source_event_id(), "calendarKey": e.calendar_key(),
        "title": e.title(), "start": ms_value(e.start()), "end": ms_value(e.end()), "isAllDay": e.is_all_day(),
        "contentKey": e.content_key(),
        "conferenceURLs": e.conferences.iter().map(|c| c.url.as_str()).collect::<Vec<_>>(),
        "otherAttendeeCount": e.other_attendee_count,
        "additionalCalendarKeys": e.additional_calendar_keys.iter().collect::<Vec<_>>(),
        "mergedMembers": e.merged_members.iter().map(|m| json!({
            "title": m.title, "calendarKey": m.calendar_key, "contentKey": m.content_key, "details": m.details,
            "start": ms_value(m.start), "end": ms_value(m.end),
        })).collect::<Vec<_>>(),
        "responseStatus": e.response_status.map_or(Value::Null, |r| json!(r.as_str())),
        "displayStart": e.display_start.map_or(Value::Null, ms_value),
        "location": e.location().map_or(Value::Null, |v| json!(v)),
        "notes": e.notes().map_or(Value::Null, |v| json!(v)),
        "url": e.url().map_or(Value::Null, |v| json!(v.as_str())),
        "mergeProvenance": provenance,
    })
}

fn decision_json(decision: &PairDecision) -> Value {
    match decision {
        PairDecision::Merge(reason) => json!({"kind": "merge", "reason": reason.as_str()}),
        PairDecision::Separate(reason) => json!({"kind": "separate", "reason": reason.as_str()}),
        PairDecision::Ambiguous => json!({"kind": "ambiguous"}),
    }
}

fn report(area: &str, total: usize, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{area}: {} of {total} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn pair_decisions_vector() {
    let file: Value = serde_json::from_str(PAIR_DECISIONS).unwrap();
    assert_eq!(file["area"], "dedup/pair-decisions");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 640);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let a = event_from_spec(&case["input"]["a"]);
        let b = event_from_spec(&case["input"]["b"]);
        let actual = json!({
            "isCandidate": DuplicateRules::is_candidate(&a, &b),
            "isManualCandidate": DuplicateRules::is_manual_candidate(&a, &b),
            "withinTimeGate": DuplicateRules::within_time_gate(&a, &b),
            "decide": decision_json(&DuplicateRules::decide(&a, &b)),
            "detailScoreA": DuplicateRules::detail_score(&a),
            "detailScoreB": DuplicateRules::detail_score(&b),
            "detailSummaryA": DuplicateRules::detail_summary(&a),
            "detailSummaryB": DuplicateRules::detail_summary(&b),
        });
        if actual != case["expected"] {
            failures.push(format!(
                "{name}: expected {}, got {actual}",
                case["expected"]
            ));
        }
    }
    report("dedup/pair-decisions", cases.len(), failures);
}

/// Runs one `resolutions.json` input and returns the same JSON shape the exporter wrote as `expected`.
pub(super) fn resolve_input(input: &Value) -> Value {
    let events: Vec<TimeTugCalendarEvent> = input["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(event_from_spec)
        .collect();
    let calendars: Vec<CalendarInfo> = input["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            CalendarInfo::new(
                c["sourceID"].as_str().unwrap(),
                c["calendarID"].as_str().unwrap(),
                c["title"].as_str().unwrap(),
            )
        })
        .collect();
    let mut book = LessonBook::default();
    for lesson in input["lessons"].as_array().unwrap() {
        book.record(
            &MergedMember::from_event(&events[lesson["a"].as_u64().unwrap() as usize]),
            &MergedMember::from_event(&events[lesson["b"].as_u64().unwrap() as usize]),
            match lesson["decision"].as_str().unwrap() {
                "same" => LessonDecision::Same,
                _ => LessonDecision::Different,
            },
            ms(&lesson["now"]),
        );
    }
    let cache = input["verdicts"].as_array().map(|list| {
        let mut filled = VerdictCache::default();
        for v in list {
            let e = &v["engine"];
            filled.store(
                &AdjudicationVerdict::new(
                    v["requestID"].as_str().unwrap(),
                    match v["answer"].as_str().unwrap() {
                        "same" => VerdictAnswer::Same,
                        "different" => VerdictAnswer::Different,
                        _ => VerdictAnswer::Unsure,
                    },
                ),
                &EngineInfo::new(
                    e["id"].as_str().unwrap(),
                    e["displayName"].as_str().unwrap(),
                    e["isOnDevice"].as_bool().unwrap(),
                ),
                ms(&v["end"]),
                ms(&v["now"]),
            );
        }
        filled
    });
    let result = DuplicateResolver::resolve_with_engine(
        &events,
        &calendars,
        &book,
        cache.as_ref(),
        input["engineID"].as_str().unwrap(),
    );
    let mut candidates = Map::new();
    for (id, list) in &result.candidates {
        candidates.insert(
            id.clone(),
            json!(list.iter().map(|e| e.id()).collect::<Vec<_>>()),
        );
    }
    json!({
        "events": result.events.iter().map(summary).collect::<Vec<_>>(),
        "pending": result.pending.iter().map(|r| json!({
            "id": r.id, "firstTitle": r.first.title, "secondTitle": r.second.title,
            "startOffsetMinutes": r.start_offset_minutes, "endOffsetMinutes": r.end_offset_minutes,
            "overlapMinutes": r.overlap_minutes, "firstDetails": r.first_details, "secondDetails": r.second_details,
            "hasConflictingDetails": r.has_conflicting_details,
        })).collect::<Vec<_>>(),
        "candidates": candidates,
        "usedLessonKeys": result.used_lesson_keys.iter().collect::<Vec<_>>(),
    })
}

#[test]
fn resolutions_vector() {
    let file: Value = serde_json::from_str(RESOLUTIONS).unwrap();
    assert_eq!(file["area"], "dedup/resolutions");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 310);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let actual = resolve_input(&case["input"]);
        if actual != case["expected"] {
            failures.push(format!(
                "{name}: resolver output differs from the Swift baseline"
            ));
        }
    }
    report("dedup/resolutions", cases.len(), failures);
}
