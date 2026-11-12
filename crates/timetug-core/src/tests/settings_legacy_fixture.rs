//! The hand-written legacy decoding of `TakeoverSettings` against the committed fixture
//! `fixtures/engine/settings-legacy.json` (`{"disabled":true,"skipSoloEvents":true,"leadTime":120}`),
//! which phase 3's `LegacyFixtureTests` also reads. The fixture is never edited.

use crate::model::takeover_settings::TakeoverSettings;

const SETTINGS_LEGACY: &str = include_str!("../../../../fixtures/engine/settings-legacy.json");

#[test]
fn the_legacy_fixture_carries_the_users_choices_forward() {
    let settings: TakeoverSettings = serde_json::from_str(SETTINGS_LEGACY).unwrap();
    assert_eq!(settings.lead_time, 120.0);
    assert!(!settings.enabled, "disabled: true becomes enabled: false");
    assert!(
        settings.require_other_attendees,
        "skipSoloEvents: true becomes requireOtherAttendees: true"
    );
    // Everything the fixture does not mention keeps its default.
    assert!(settings.takeover_calendar_keys.is_empty());
    assert!(settings.hidden_calendar_keys.is_empty());
    assert!(!settings.require_conference_link);
    assert!(settings.skip_all_day_events);
}

#[test]
fn saving_the_migrated_fixture_writes_only_the_current_keys() {
    let settings: TakeoverSettings = serde_json::from_str(SETTINGS_LEGACY).unwrap();
    let written = serde_json::to_string(&settings).unwrap();
    assert!(
        !written.contains("disabled") && !written.contains("skipSoloEvents"),
        "{written}"
    );
    // A whole number is written without a fraction (C# reads the file into an integer).
    assert!(
        written.contains(r#""leadTime":120,"#) || written.contains(r#""leadTime":120}"#),
        "{written}"
    );
    let reloaded: TakeoverSettings = serde_json::from_str(&written).unwrap();
    assert_eq!(reloaded, settings);
}

#[test]
fn a_legacy_flag_of_the_wrong_type_is_read_only_when_the_new_key_is_absent() {
    // Swift's `??` chain reads `skipSoloEvents` only when `requireOtherAttendees` is missing, so a malformed legacy key
    // next to a good new key is ignored, and next to nothing it is an error.
    let ok: TakeoverSettings = serde_json::from_str(
        r#"{"requireOtherAttendees":true,"enabled":false,"skipSoloEvents":"yes","disabled":[1]}"#,
    )
    .unwrap();
    assert!(ok.require_other_attendees && !ok.enabled);
    assert!(serde_json::from_str::<TakeoverSettings>(r#"{"skipSoloEvents":"yes"}"#).is_err());
    assert!(serde_json::from_str::<TakeoverSettings>(r#"{"disabled":1}"#).is_err());
    // `null` is a missing key, as `decodeIfPresent` treats it.
    let null: TakeoverSettings =
        serde_json::from_str(r#"{"enabled":null,"leadTime":null,"disabled":null}"#).unwrap();
    assert_eq!(null, TakeoverSettings::default());
}
