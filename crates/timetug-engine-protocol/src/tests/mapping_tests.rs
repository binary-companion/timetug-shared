//! Port of `MappingTests.swift` (including its `ReviewFixTests` suite).

use std::collections::BTreeSet;

use calendar_core::time::{Zone, add_seconds, seconds_between};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde_json::{Value, json};
use timetug_core::agenda::day_agenda::DayAgenda;
use timetug_core::dedup::adjudication::EngineInfo;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::{
    InferenceStatus, MergeProvenance, MergedMember, SourceStatus, TakeoverSettings,
};
use timetug_engine::diagnostic::{EngineDiagnostic, Level};
use timetug_engine::event::EngineEvent;
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::EngineState;

use super::support::{fixture, ts};
use crate::service_messages::{ReplyErrorKind, WireReply};
use crate::state_mapping::{wire_message_for_event, wire_message_for_state};
use crate::wire_types::{
    WireDates, WireEvent, WireInferenceStatus, WireProvenance, WireSettings, WireSettingsPatch,
    WireSourceStatus, WireState,
};

/// `Date(timeIntervalSince1970: seconds)`.
fn date(seconds: f64) -> Timestamp {
    add_seconds(Timestamp::UNIX_EPOCH, seconds)
}

#[test]
fn every_engine_state_property_has_a_wire_field() {
    // Rust has no reflection: destructuring `EngineState` without `..` stops compiling when the engine adds a field,
    // and the names below are then compared with the wire state's own list (`WireState::FIELD_NAMES`).
    let EngineState {
        revision: _,
        agenda: _,
        events: _,
        calendars: _,
        statuses: _,
        notices: _,
        source_names: _,
        candidates: _,
        accounts: _,
        account_status: _,
        settings: _,
        inference_status: _,
        ledger: _,
        next_takeover: _,
        takeover_visible: _,
        available_account_kinds: _,
    } = EngineState::empty();
    let engine: BTreeSet<&str> = [
        "revision",
        "agenda",
        "events",
        "calendars",
        "statuses",
        "notices",
        "sourceNames",
        "candidates",
        "accounts",
        "accountStatus",
        "settings",
        "inferenceStatus",
        "ledger",
        "nextTakeover",
        "takeoverVisible",
        "availableAccountKinds",
    ]
    .into();
    let wire: BTreeSet<&str> = WireState::FIELD_NAMES.iter().copied().collect();
    assert_eq!(WireState::FIELD_NAMES.len(), wire.len(), "duplicate field");
    assert!(
        engine.difference(&wire).next().is_none(),
        "wire state is missing engine fields"
    );
    assert!(
        wire.difference(&engine).next().is_none(),
        "wire state has fields the engine does not"
    );
}

#[test]
fn empty_state_encodes_every_field_name() {
    let envelope = wire_message_for_state(&EngineState::empty());
    assert_eq!(envelope.r#type, "state");
    let payload = envelope.payload.expect("no payload");
    let keys: BTreeSet<&str> = payload
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    // nextTakeover is absent when nil; every other field is present.
    let expected: BTreeSet<&str> = WireState::FIELD_NAMES
        .iter()
        .copied()
        .filter(|name| *name != "nextTakeover")
        .collect();
    assert_eq!(keys, expected);
}

#[test]
fn all_day_event_uses_dates_not_instants() {
    let wire = WireEvent::new(&fixture::all_day("ad1", &fixture::berlin()));
    assert_eq!(wire.start_date.as_deref(), Some("2026-10-16"));
    assert_eq!(wire.end_date.as_deref(), Some("2026-10-17"));
    assert!(wire.start.is_none() && wire.end.is_none());
    assert_eq!(wire.time_zone, "Europe/Berlin");
    assert!(wire.is_all_day);
}

#[test]
fn all_day_dates_follow_the_event_zone_not_utc() {
    let zone = Zone::named("Pacific/Auckland").unwrap();
    let wire = WireEvent::new(&fixture::all_day("ad1", &zone));
    assert_eq!(wire.start_date.as_deref(), Some("2026-10-16"));
    assert_eq!(wire.end_date.as_deref(), Some("2026-10-17"));
    assert_eq!(wire.time_zone, "Pacific/Auckland");
}

#[test]
fn timed_event_carries_instants_and_no_dates() {
    let event = fixture::timed_with(
        "e1",
        "Design review",
        fixture::default_start(),
        30,
        "cal",
        "fake",
        Some("https://zoom.us/j/123"),
    );
    let wire = WireEvent::new(&event);
    assert_eq!(wire.start.as_deref(), Some("2025-10-09T08:53:20.000Z"));
    assert_eq!(wire.end.as_deref(), Some("2025-10-09T09:23:20.000Z"));
    assert!(wire.start_date.is_none());
    assert_eq!(wire.time_zone, "Europe/Berlin");
    assert_eq!(
        wire.conferences
            .iter()
            .map(|c| c.url.as_str())
            .collect::<Vec<_>>(),
        ["https://zoom.us/j/123"]
    );
}

#[test]
fn event_with_no_conference_link_has_empty_list() {
    assert!(WireEvent::new(&fixture::timed("e1")).conferences.is_empty());
}

#[test]
fn merged_members_are_listed_by_content_key() {
    let mut event = fixture::timed("e1");
    let member = MergedMember::new(
        "Design review",
        "fake/other",
        "design review|1760000000|1760001800",
        "bare",
        event.start(),
        add_seconds(event.end(), 600.0),
    );
    event.merged_members = vec![member.clone()];
    event.merge_provenance = Some(MergeProvenance::Inference {
        engine_id: "m".into(),
        engine_name: "Model".into(),
    });
    let wire = WireEvent::new(&event);
    assert_eq!(wire.members.len(), 1);
    assert_eq!(wire.members[0].member_id, member.content_key);
    assert_eq!(wire.members[0].end, "2025-10-09T09:33:20.000Z");
    assert_eq!(
        wire.merge_provenance,
        Some(WireProvenance {
            kind: "inference".into(),
            engine_id: Some("m".into()),
            engine_name: Some("Model".into()),
        })
    );
}

#[test]
fn dates_are_utc_milliseconds() {
    assert_eq!(
        WireDates::string(date(1_760_000_000.123)),
        "2025-10-09T08:53:20.123Z"
    );
    assert_eq!(WireDates::string(date(0.0)), "1970-01-01T00:00:00.000Z");
    assert_eq!(WireDates::string(date(-1.5)), "1969-12-31T23:59:58.500Z");
    assert_eq!(
        WireDates::string(date(951_782_400.0)),
        "2000-02-29T00:00:00.000Z"
    );
}

#[test]
fn dates_parse_what_they_write() {
    for t in [0.0, 1_760_000_000.123, -86_400.25, 4_102_444_800.0] {
        let d = date(t);
        let parsed = WireDates::date(&WireDates::string(d)).expect("parses");
        assert!(seconds_between(d, parsed).abs() < 0.001, "{t}");
    }
    assert_eq!(
        WireDates::date("2025-10-09T08:53:20Z"),
        Some(ts(1_760_000_000))
    );
    assert_eq!(WireDates::date("garbage"), None);
    assert_eq!(WireDates::date("2025-13-09T08:53:20Z"), None);
}

#[test]
fn diagnostic_carries_no_payload_id() {
    let env = wire_message_for_event(&EngineEvent::Diagnostic(EngineDiagnostic::new(
        Level::Info,
        "x",
        "m",
    )));
    assert_eq!(env.r#type, "event.diagnostic");
    assert!(env.id.is_none());
    assert_eq!(
        env.payload,
        Some(json!({"level": "info", "category": "x", "message": "m"}))
    );
}

#[test]
fn takeover_due_flattens_the_request_and_calendar_title() {
    let event = fixture::timed_with(
        "e1",
        "Design review",
        fixture::default_start(),
        30,
        "cal",
        "fake",
        Some("https://zoom.us/j/9"),
    );
    let request = TakeoverRequest::make(&event, add_seconds(event.start(), -30.0));
    let env = wire_message_for_event(&EngineEvent::TakeoverDue {
        request,
        calendar_title: Some("Work".into()),
    });
    assert_eq!(env.r#type, "event.takeoverDue");
    let payload = env.payload.unwrap();
    assert_eq!(payload["calendarTitle"], json!("Work"));
    assert_eq!(payload["event"]["title"], json!("Design review"));
    assert_eq!(payload["hasStarted"], json!(false));
    assert_eq!(payload["joinURL"], json!("https://zoom.us/j/9"));
}

#[test]
fn simple_events_map_their_payloads() {
    let withdrawn = wire_message_for_event(&EngineEvent::TakeoverWithdrawn {
        event_id: "e1".into(),
    });
    assert_eq!(withdrawn.r#type, "event.takeoverWithdrawn");
    assert_eq!(withdrawn.payload, Some(json!({"eventID": "e1"})));
    let join = wire_message_for_event(&EngineEvent::JoinRequested(
        UrlString::parse("https://meet.example/x").unwrap(),
    ));
    assert_eq!(join.r#type, "event.joinRequested");
    assert_eq!(join.payload, Some(json!({"url": "https://meet.example/x"})));
}

#[test]
fn source_statuses_map_each_kind() {
    assert_eq!(
        WireSourceStatus::new(&SourceStatus::Ok),
        WireSourceStatus {
            kind: "ok".into(),
            message: None
        }
    );
    assert_eq!(
        WireSourceStatus::new(&SourceStatus::NeedsPermission).kind,
        "needsPermission"
    );
    assert_eq!(
        WireSourceStatus::new(&SourceStatus::AuthExpired).kind,
        "authExpired"
    );
    assert_eq!(
        WireSourceStatus::new(&SourceStatus::Failing("boom".into())),
        WireSourceStatus {
            kind: "failing".into(),
            message: Some("boom".into())
        }
    );
}

#[test]
fn inference_status_maps_each_kind() {
    assert_eq!(
        WireInferenceStatus::new(&InferenceStatus::Disabled).kind,
        "disabled"
    );
    assert_eq!(
        WireInferenceStatus::new(&InferenceStatus::NoEngine).kind,
        "noEngine"
    );
    assert_eq!(
        WireInferenceStatus::new(&InferenceStatus::NotOnDevice).kind,
        "notOnDevice"
    );
    assert_eq!(
        WireInferenceStatus::new(&InferenceStatus::Unavailable { reason: "r".into() })
            .reason
            .as_deref(),
        Some("r")
    );
    let active =
        WireInferenceStatus::new(&InferenceStatus::Active(EngineInfo::new("e", "N", true)));
    assert!(
        active.kind == "active"
            && active.engine_id.as_deref() == Some("e")
            && active.engine_name.as_deref() == Some("N")
            && active.is_on_device == Some(true)
    );
}

#[test]
fn full_state_maps_collections_by_id() {
    let a = fixture::timed("a");
    let mut b = fixture::timed("b");
    b.set_title("Design review ");
    let mut state = EngineState::empty();
    state.revision = 7;
    state.events = vec![a.clone(), b.clone()];
    state.candidates = [(a.id(), vec![b.clone()])].into();
    state.statuses = [
        ("s1".to_string(), SourceStatus::Failing("x".into())),
        ("s2".to_string(), SourceStatus::Ok),
    ]
    .into();
    state.agenda = DayAgenda::make(
        std::slice::from_ref(&a),
        &TakeoverSettings::default(),
        add_seconds(a.start(), -60.0),
        &fixture::utc(),
    );
    let wire = WireState::new(&state);
    assert_eq!(wire.revision, 7);
    assert_eq!(
        wire.events.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        [a.id(), b.id()]
    );
    assert_eq!(wire.candidates, [(a.id(), vec![b.id()])].into());
    assert_eq!(wire.statuses["s1"].kind, "failing");
    assert_eq!(
        wire.agenda
            .items
            .iter()
            .map(|i| i.event_id.clone())
            .collect::<Vec<_>>(),
        [a.id()]
    );
    assert_eq!(wire.agenda.next_event_id, Some(a.id()));
}

#[test]
fn settings_map_and_patch_leaves_absent_fields_alone() {
    let mut settings = EngineSettings::default();
    settings.takeover.takeover_calendar_keys = ["b/1".to_string(), "a/1".to_string()].into();
    let wire = WireSettings::new(&settings);
    assert_eq!(wire.takeover.takeover_calendar_keys, ["a/1", "b/1"]);

    let mut patch = WireSettingsPatch::default();
    patch.inference_enabled = Some(true);
    patch.lead_time = Some(300.0);
    let mut changed = settings.clone();
    patch.apply_to(&mut changed);
    assert!(changed.inference_enabled);
    assert_eq!(changed.takeover.lead_time, 300.0);
    assert_eq!(
        changed.takeover.takeover_calendar_keys,
        settings.takeover.takeover_calendar_keys
    );
    assert_eq!(
        changed.system_calendar_enabled,
        settings.system_calendar_enabled
    );

    let mut empty = settings.clone();
    WireSettingsPatch::default().apply_to(&mut empty);
    assert_eq!(empty, settings);
}

#[test]
fn hiding_a_calendar_removes_its_takeover_opt_in() {
    let mut settings = EngineSettings::default();
    settings.takeover.takeover_calendar_keys = ["a/1".to_string()].into();
    let mut patch = WireSettingsPatch::default();
    patch.hidden_calendar_keys = Some(vec!["a/1".to_string()]);
    patch.apply_to(&mut settings);
    assert!(settings.takeover.takeover_calendar_keys.is_empty());
}

#[test]
fn reply_parsing_never_throws() {
    assert_eq!(
        WireReply::parse(r#"{"v":1,"result":{"a":1}}"#),
        WireReply::Result(json!({"a": 1}))
    );
    assert_eq!(
        WireReply::parse(r#"{"v":1,"error":{"kind":"denied","message":"m"}}"#),
        WireReply::Error {
            kind: ReplyErrorKind::Denied,
            message: "m".into()
        }
    );
    assert_eq!(
        WireReply::parse(r#"{"v":1,"error":{"kind":"weird"}}"#),
        WireReply::Error {
            kind: ReplyErrorKind::Failed,
            message: String::new()
        }
    );
    assert_eq!(
        WireReply::parse(r#"{"v":2,"result":1}"#),
        WireReply::Error {
            kind: ReplyErrorKind::Failed,
            message: "unsupported reply version".into()
        }
    );
    assert_eq!(
        WireReply::parse("{nope"),
        WireReply::Error {
            kind: ReplyErrorKind::Failed,
            message: "reply is not a JSON object".into()
        }
    );
    assert_eq!(
        WireReply::parse(r#"{"v":1}"#),
        WireReply::Result(Value::Null)
    );
}

// MARK: - ReviewFixTests

#[test]
fn long_fraction_is_nil_not_a_trap() {
    assert_eq!(
        WireDates::date("2025-10-09T08:53:20.12345678901234567890Z"),
        None
    );
}

#[test]
fn impossible_calendar_date_is_nil() {
    assert_eq!(WireDates::date("2025-02-30T00:00:00.000Z"), None);
    assert_eq!(WireDates::date("2025-04-31T00:00:00.000Z"), None);
    assert!(WireDates::date("2024-02-29T00:00:00.000Z").is_some());
}

#[test]
fn numbers_stay_numbers() {
    let v: Value = serde_json::from_str(r#"{"a":1,"b":0,"c":true}"#).unwrap();
    assert_eq!(v["a"], json!(1));
    assert_eq!(v["b"], json!(0));
    assert_eq!(v["c"], json!(true));
}

#[test]
fn malformed_error_member_is_a_failure_not_a_success() {
    let reply = WireReply::parse(r#"{"v":1,"error":"boom"}"#);
    match reply {
        WireReply::Error { kind, .. } => assert_eq!(kind, ReplyErrorKind::Failed),
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn the_extremes_of_a_timestamp_format_without_overflow() {
    assert!(WireDates::string(Timestamp::MIN).ends_with("Z"));
    let max = WireDates::string(Timestamp::MAX);
    assert!(max.starts_with("9999-12-30T"), "{max}");
    assert!(WireDates::date("9999-12-31T23:59:59.999Z").is_none());
    assert!(WireDates::unix_nanoseconds("9999-12-31T23:59:59.999Z").is_some());
}
