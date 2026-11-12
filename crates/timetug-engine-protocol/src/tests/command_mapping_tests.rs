//! Port of `CommandMappingTests.swift`.

use serde_json::{Value, json};
use timetug_core::MergedMember;
use timetug_engine::command::{EngineCommand, SystemEvent};
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::EngineState;

use super::support::fixture;
use crate::command_mapping::{CommandDecoder, CommandOutcome};
use crate::envelope::{Envelope, MessageId};

fn decode_full(
    kind: &str,
    payload: Option<Value>,
    id: Option<MessageId>,
    v: i64,
    testing: bool,
    state: &EngineState,
) -> CommandOutcome {
    let envelope = Envelope {
        v,
        r#type: kind.to_string(),
        id,
        payload,
    };
    CommandDecoder::new(testing).decode(&envelope, state)
}

fn decode(kind: &str, payload: Option<Value>) -> CommandOutcome {
    decode_full(kind, payload, None, 1, false, &EngineState::empty())
}

fn command(outcome: CommandOutcome) -> Option<EngineCommand> {
    match outcome {
        CommandOutcome::Engine(c) => Some(c),
        _ => None,
    }
}

fn reason(outcome: CommandOutcome) -> Option<String> {
    match outcome {
        CommandOutcome::Rejected(d) => {
            assert_eq!(d.category, "command");
            Some(d.message)
        }
        _ => None,
    }
}

#[test]
fn merge_with_unknown_id_is_rejected_with_diagnostic() {
    let out = decode_full(
        "dedup.merge",
        Some(json!({"eventIDs": ["nope", "nada"]})),
        Some(MessageId::String("c1".into())),
        1,
        false,
        &EngineState::empty(),
    );
    assert_eq!(reason(out).as_deref(), Some("unknown event id"));
}

#[test]
fn merge_resolves_ids_in_events_and_candidates() {
    let a = fixture::timed("a");
    let mut b = fixture::timed("b");
    b.set_title("Other");
    let mut state = EngineState::empty();
    state.events = vec![a.clone()];
    state.candidates = [(a.id(), vec![b.clone()])].into();
    let out = decode_full(
        "dedup.merge",
        Some(json!({"eventIDs": [a.id(), b.id()]})),
        None,
        1,
        false,
        &state,
    );
    match command(out) {
        Some(EngineCommand::Merge(x, y)) => assert!(x.id() == a.id() && y.id() == b.id()),
        other => panic!("not a merge: {other:?}"),
    }
}

#[test]
fn merge_needs_exactly_two_ids() {
    let out = decode("dedup.merge", Some(json!({"eventIDs": ["a"]})));
    assert_eq!(reason(out).as_deref(), Some("invalid payload"));
}

#[test]
fn separate_resolves_members_by_content_key() {
    let mut merged = fixture::timed("m");
    let member = MergedMember::new("t", "c", "k1", "bare", merged.start(), merged.end());
    merged.merged_members = vec![member.clone()];
    let mut state = EngineState::empty();
    state.events = vec![merged.clone()];
    let ok = decode_full(
        "dedup.separate",
        Some(json!({"eventID": merged.id(), "memberIDs": ["k1"]})),
        None,
        1,
        false,
        &state,
    );
    match command(ok) {
        Some(EngineCommand::Separate { event, members }) => {
            assert!(event.id() == merged.id() && members == vec![member]);
        }
        other => panic!("not a separate: {other:?}"),
    }
    let bad = decode_full(
        "dedup.separate",
        Some(json!({"eventID": merged.id(), "memberIDs": ["zz"]})),
        None,
        1,
        false,
        &state,
    );
    assert_eq!(reason(bad).as_deref(), Some("unknown member id"));
}

#[test]
fn settings_patch_leaves_absent_fields_alone() {
    let out = decode("settings.apply", Some(json!({"inferenceEnabled": true})));
    let Some(EngineCommand::ApplySettings(f)) = command(out) else {
        panic!("not applySettings");
    };
    let mut s = EngineSettings::default();
    s.system_calendar_enabled = false;
    f(&mut s);
    assert!(s.inference_enabled);
    assert!(!s.system_calendar_enabled);
}

#[test]
fn settings_apply_with_no_payload_is_a_no_op() {
    let Some(EngineCommand::ApplySettings(f)) = command(decode("settings.apply", None)) else {
        panic!("rejected");
    };
    let mut s = EngineSettings::default();
    f(&mut s);
    assert_eq!(s, EngineSettings::default());
}

#[test]
fn advance_clock_requires_testing_flag() {
    let payload = json!({"seconds": 60});
    assert_eq!(
        reason(decode("test.advanceClock", Some(payload.clone()))).as_deref(),
        Some("test commands are disabled")
    );
    let accepted = decode_full(
        "test.advanceClock",
        Some(payload),
        None,
        1,
        true,
        &EngineState::empty(),
    );
    match accepted {
        CommandOutcome::TestAdvanceClock { seconds } => assert_eq!(seconds, 60.0),
        _ => panic!("not accepted"),
    }
    let negative = decode_full(
        "test.advanceClock",
        Some(json!({"seconds": -1})),
        None,
        1,
        true,
        &EngineState::empty(),
    );
    assert_eq!(reason(negative).as_deref(), Some("invalid payload"));
}

#[test]
fn simple_commands_map() {
    assert!(matches!(
        command(decode("start", None)),
        Some(EngineCommand::Start)
    ));
    assert!(matches!(
        command(decode("refresh", None)),
        Some(EngineCommand::Refresh)
    ));
    assert!(matches!(
        command(decode("account.cancel", None)),
        Some(EngineCommand::CancelAuthorization)
    ));
    assert!(matches!(
        command(decode("dedup.forget", None)),
        Some(EngineCommand::ForgetCorrections)
    ));
    match command(decode("account.add", Some(json!({"kindID": "google"})))) {
        Some(EngineCommand::AddAccount { kind_id }) => assert_eq!(kind_id, "google"),
        other => panic!("add: {other:?}"),
    }
    match command(decode(
        "account.reconnect",
        Some(json!({"connectionID": "x"})),
    )) {
        Some(EngineCommand::ReauthorizeAccount { connection_id }) => {
            assert_eq!(connection_id, "x")
        }
        other => panic!("reconnect: {other:?}"),
    }
    match command(decode("account.remove", Some(json!({"connectionID": "y"})))) {
        Some(EngineCommand::RemoveAccount { connection_id }) => assert_eq!(connection_id, "y"),
        other => panic!("remove: {other:?}"),
    }
    assert!(matches!(
        command(decode("countdown.visible", Some(json!({"visible": true})))),
        Some(EngineCommand::SetCountdownVisible(true))
    ));
}

#[test]
fn takeover_commands_map() {
    let id = json!("e1");
    match command(decode(
        "takeover.presented",
        Some(json!({"eventID": id, "onScreen": false})),
    )) {
        Some(EngineCommand::TakeoverPresented {
            event_id,
            on_screen,
        }) => assert!(event_id == "e1" && !on_screen),
        other => panic!("presented: {other:?}"),
    }
    match command(decode("takeover.closed", Some(json!({"eventID": id})))) {
        Some(EngineCommand::TakeoverClosed { event_id }) => assert_eq!(event_id, "e1"),
        other => panic!("closed: {other:?}"),
    }
    match command(decode("takeover.join", Some(json!({"eventID": id})))) {
        Some(EngineCommand::Join { event_id }) => assert_eq!(event_id, "e1"),
        other => panic!("join: {other:?}"),
    }
    match command(decode("takeover.dismiss", Some(json!({"eventID": id})))) {
        Some(EngineCommand::Dismiss { event_id }) => assert_eq!(event_id, "e1"),
        other => panic!("dismiss: {other:?}"),
    }
    match command(decode(
        "takeover.snooze",
        Some(json!({"eventID": id, "seconds": 300})),
    )) {
        Some(EngineCommand::Snooze { event_id, seconds }) => {
            assert!(event_id == "e1" && seconds == 300.0)
        }
        other => panic!("snooze: {other:?}"),
    }
    match command(decode(
        "takeover.test",
        Some(json!({"title": "T", "calendarTitle": "C"})),
    )) {
        Some(EngineCommand::TestTug {
            title,
            calendar_title,
        }) => assert!(title == "T" && calendar_title == "C"),
        other => panic!("test: {other:?}"),
    }
}

#[test]
fn system_event_kinds_map_and_unknown_is_rejected() {
    for (kind, expected) in [
        ("wake", SystemEvent::Wake),
        ("clockChanged", SystemEvent::ClockChanged),
        ("dayChanged", SystemEvent::DayChanged),
        ("networkReturned", SystemEvent::NetworkReturned),
        ("displayChanged", SystemEvent::DisplayChanged),
    ] {
        match command(decode("system.event", Some(json!({"kind": kind})))) {
            Some(EngineCommand::System(e)) => assert_eq!(e, expected, "{kind}"),
            other => panic!("{kind}: {other:?}"),
        }
    }
    assert_eq!(
        reason(decode("system.event", Some(json!({"kind": "explode"})))).as_deref(),
        Some("unknown system event kind")
    );
}

#[test]
fn time_zone_change_carries_the_zone() {
    let out = decode(
        "system.event",
        Some(json!({"kind": "timeZoneChanged", "timeZone": "Asia/Tokyo"})),
    );
    match out {
        CommandOutcome::TimeZone(zone, EngineCommand::System(SystemEvent::TimeZoneChanged)) => {
            assert_eq!(zone.identifier(), "Asia/Tokyo")
        }
        _ => panic!("not a zone change"),
    }
    assert!(matches!(
        command(decode(
            "system.event",
            Some(json!({"kind": "timeZoneChanged"}))
        )),
        Some(EngineCommand::System(SystemEvent::TimeZoneChanged))
    ));
    let bad = decode(
        "system.event",
        Some(json!({"kind": "timeZoneChanged", "timeZone": "Mars/X"})),
    );
    assert_eq!(reason(bad).as_deref(), Some("unknown time zone"));
}

#[test]
fn rejects_unknown_type_wrong_version_and_bad_payload_without_throwing() {
    assert_eq!(
        reason(decode("nope", None)).as_deref(),
        Some("unknown command type")
    );
    assert_eq!(
        reason(decode("stop", None)).as_deref(),
        Some("unknown command type")
    );
    let v2 = decode_full("refresh", None, None, 2, false, &EngineState::empty());
    assert_eq!(reason(v2).as_deref(), Some("unsupported protocol version"));
    assert_eq!(
        reason(decode("account.add", Some(json!({"kindID": 3})))).as_deref(),
        Some("invalid payload")
    );
    assert_eq!(
        reason(decode("takeover.join", Some(json!("not an object")))).as_deref(),
        Some("invalid payload")
    );
    assert_eq!(
        reason(decode(
            "takeover.snooze",
            Some(json!({"eventID": "e", "seconds": -5}))
        ))
        .as_deref(),
        Some("invalid payload")
    );
}
