//! Port of `SettingsRulesTests.swift`.

use crate::model::takeover_settings::TakeoverSettings;

fn decode(json: &str) -> TakeoverSettings {
    serde_json::from_str(json).unwrap()
}

#[test]
fn enabling_takeover_shows_hidden_calendar() {
    let mut s = TakeoverSettings::default();
    s.hidden_calendar_keys = ["a".to_string()].into();
    s.set_takeover(true, "a");
    assert!(s.is_shown_in_list("a"));
    assert!(s.takeover_calendar_keys.contains("a"));
}

#[test]
fn hiding_calendar_turns_takeover_off() {
    let mut s = TakeoverSettings::default();
    s.set_takeover(true, "a");
    s.set_shown_in_list(false, "a");
    assert!(!s.is_shown_in_list("a"));
    assert!(!s.takeover_calendar_keys.contains("a"));
}

#[test]
fn showing_calendar_does_not_enable_takeover() {
    let mut s = TakeoverSettings::default();
    s.set_shown_in_list(false, "a");
    s.set_shown_in_list(true, "a");
    assert!(s.is_shown_in_list("a"));
    assert!(!s.takeover_calendar_keys.contains("a"));
}

#[test]
fn disabling_takeover_leaves_calendar_shown() {
    let mut s = TakeoverSettings::default();
    s.set_takeover(true, "a");
    s.set_takeover(false, "a");
    assert!(s.is_shown_in_list("a"));
    assert!(!s.takeover_calendar_keys.contains("a"));
}

#[test]
fn decoding_conflict_lets_hiding_win() {
    // A calendar that is not shown never tugs.
    let s = decode(r#"{"takeoverCalendarKeys":["a","c"],"hiddenCalendarKeys":["a","b"]}"#);
    assert!(!s.is_shown_in_list("a"));
    assert!(!s.takeover_calendar_keys.contains("a"));
    assert!(s.takeover_calendar_keys.contains("c"));
    assert!(!s.is_shown_in_list("b"));
}

#[test]
fn enabled_defaults_to_true_when_missing_from_saved_json() {
    assert!(decode("{}").enabled);
}

#[test]
fn enabled_round_trips_through_json() {
    let mut settings = TakeoverSettings::default();
    settings.enabled = false;
    let decoded: TakeoverSettings =
        serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(!decoded.enabled);
}

#[test]
fn legacy_disabled_flag_migrates_to_enabled() {
    assert!(!decode(r#"{"disabled":true}"#).enabled);
    assert!(decode(r#"{"disabled":false}"#).enabled);
    // The new key wins when both are present.
    assert!(decode(r#"{"disabled":true,"enabled":true}"#).enabled);
}

#[test]
fn legacy_skip_solo_flag_migrates_to_require_other_attendees() {
    assert!(decode(r#"{"skipSoloEvents":true}"#).require_other_attendees);
    assert!(!decode(r#"{"skipSoloEvents":false}"#).require_other_attendees);
    assert!(
        !decode(r#"{"skipSoloEvents":true,"requireOtherAttendees":false}"#).require_other_attendees
    );
}

#[test]
fn saved_settings_no_longer_write_retired_keys() {
    let json = serde_json::to_string(&TakeoverSettings::default()).unwrap();
    assert!(
        !json.contains("disabled")
            && !json.contains("skipSoloEvents")
            && !json.contains("skipDeclinedEvents")
    );
}

#[test]
fn setting_tug_for_several_calendars_also_shows_them_and_clears_it() {
    let mut s = TakeoverSettings::default();
    s.hidden_calendar_keys = ["a".to_string(), "b".to_string()].into();
    s.set_takeover_for_calendars(true, &["a", "b", "c"]);
    assert_eq!(
        s.takeover_calendar_keys,
        ["a".to_string(), "b".to_string(), "c".to_string()].into()
    );
    assert!(s.hidden_calendar_keys.is_empty());
    s.set_takeover_for_calendars(false, &["a", "b"]);
    assert_eq!(s.takeover_calendar_keys, ["c".to_string()].into());
    assert!(s.is_shown_in_list("a"));
}
