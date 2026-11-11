//! Port of `TakeoverSettingsRemovalTests.swift`.

use crate::model::takeover_settings::TakeoverSettings;

fn set(keys: &[&str]) -> std::collections::BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

#[test]
fn remove_calendars_for_source_drops_only_that_sources_keys() {
    let mut s = TakeoverSettings::default();
    s.takeover_calendar_keys = set(&["google-1/a", "google-10/a", "eventkit/x"]);
    s.hidden_calendar_keys = set(&["google-1/b", "eventkit/y"]);
    s.remove_calendars_for_source_id("google-1");
    assert_eq!(
        s.takeover_calendar_keys,
        set(&["google-10/a", "eventkit/x"])
    );
    assert_eq!(s.hidden_calendar_keys, set(&["eventkit/y"]));
}

#[test]
fn remove_calendars_for_unknown_source_changes_nothing() {
    let mut s = TakeoverSettings::default();
    s.takeover_calendar_keys = set(&["eventkit/x"]);
    s.remove_calendars_for_source_id("google-1");
    assert_eq!(s.takeover_calendar_keys, set(&["eventkit/x"]));
}

#[test]
fn remove_calendars_where_source_matches_predicate_uses_text_before_first_slash() {
    let mut s = TakeoverSettings::default();
    s.takeover_calendar_keys = set(&["google-a/cal/with/slashes", "google-b/c", "eventkit/x"]);
    s.remove_calendars_where_source_id(|id| id.starts_with("google-") && id != "google-b");
    assert_eq!(s.takeover_calendar_keys, set(&["google-b/c", "eventkit/x"]));
}
