//! `Tests/TimeTugEngineTests/Fixtures/takeover-ledger.json`: a ledger file as the Swift engine wrote it (family A:
//! dates are seconds since 2001-01-01). The store that reads the file is phase 3; the format is pinned here.
//! The fixture is never edited.

use calendar_core::json::swift_reference_date;
use serde_json::Value;

use super::support::EventSpec;
use super::support::make_event_with;
use crate::takeover::ledger::{Entry, TakeoverLedger};

const FIXTURE: &str =
    include_str!("../../../../Tests/TimeTugEngineTests/Fixtures/takeover-ledger.json");

/// The event whose keys are `s/<id>/1800000000` and `sample meeting <id>|1800000000|1800001800`.
fn sample(id: &str) -> crate::model::event::TimeTugCalendarEvent {
    let mut event = make_event_with(EventSpec {
        id: id.to_string(),
        title: format!("Sample Meeting {}", id.to_uppercase()),
        start: "2027-01-15T08:00:00Z".to_string(),
        minutes: 30,
        ..EventSpec::default()
    });
    event.source_id = "s".to_string();
    event
}

fn reference(seconds: f64) -> jiff::Timestamp {
    swift_reference_date::from_number(seconds).unwrap()
}

#[test]
fn fixture_loads_to_the_expected_values() {
    let ledger: TakeoverLedger = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(ledger.key_count(), 4);
    let a = sample("a");
    let b = sample("b");
    // The fixture's instants: 1800000000 and 1800001800 Unix seconds are 821692800 and 821694600 reference seconds.
    assert_eq!(a.id(), "s/a/1800000000");
    assert_eq!(a.content_key(), "sample meeting a|1800000000|1800001800");
    assert_eq!(reference(821_694_600.0), a.end());
    assert_eq!(ledger.entry_for(&a), Some(Entry::Fired));
    assert_eq!(
        ledger.entry_for(&b),
        Some(Entry::Snoozed {
            until: reference(821_692_500.0)
        })
    );
    assert!(ledger.has_fired(&a));
    assert!(ledger.is_snoozed(&b));
    // Pruning at the recorded time keeps all four; one second after the meetings end drops all four.
    let mut kept = ledger.clone();
    assert!(!kept.prune(reference(821_692_300.0)));
    let mut dropped = ledger.clone();
    assert!(dropped.prune(reference(821_694_601.0)));
    assert_eq!(dropped.key_count(), 0);
}

#[test]
fn fixture_saves_to_the_same_json_and_reloads_equal() {
    let ledger: TakeoverLedger = serde_json::from_str(FIXTURE).unwrap();
    let saved = serde_json::to_string_pretty(&ledger).unwrap();
    let fixture_value: Value = serde_json::from_str(FIXTURE).unwrap();
    let saved_value: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved_value, fixture_value);
    let reloaded: TakeoverLedger = serde_json::from_str(&saved).unwrap();
    assert_eq!(reloaded, ledger);
}

#[test]
fn dates_are_written_as_reference_date_numbers_without_a_fraction() {
    let ledger: TakeoverLedger = serde_json::from_str(FIXTURE).unwrap();
    let saved = serde_json::to_string(&ledger).unwrap();
    assert!(saved.contains("\"recordedAt\":821692200"), "{saved}");
    assert!(saved.contains("\"until\":821692500"), "{saved}");
    assert!(saved.contains("\"fired\":{}"), "{saved}");
    assert!(!saved.contains("821692200.0"), "{saved}");
}

#[test]
fn malformed_records_are_dropped_and_malformed_files_are_errors() {
    // Swift: a record that is not decodable is dropped (`LenientRecord`); the file itself must have `records`.
    let text = r#"{"records":{
        "ok": {"entry":{"fired":{}},"end":10,"recordedAt":5},
        "null": null,
        "number": 3,
        "array": [],
        "two-cases": {"entry":{"fired":{},"snoozed":{"until":1}},"end":10,"recordedAt":5},
        "no-case": {"entry":{},"end":10,"recordedAt":5},
        "unknown-case": {"entry":{"unknown":{}},"end":10,"recordedAt":5},
        "fired-not-object": {"entry":{"fired":true},"end":10,"recordedAt":5},
        "snoozed-without-until": {"entry":{"snoozed":{}},"end":10,"recordedAt":5},
        "end-is-text": {"entry":{"fired":{}},"end":"10","recordedAt":5},
        "no-recorded-at": {"entry":{"fired":{}},"end":10},
        "extra-keys-ignored": {"entry":{"fired":{"x":1},"other":2},"end":10,"recordedAt":5,"more":true}
    }}"#;
    let ledger: TakeoverLedger = serde_json::from_str(text).unwrap();
    assert_eq!(ledger.key_count(), 2);
    for bad in [
        r#"{}"#,
        r#"{"records":[]}"#,
        r#"{"records":null}"#,
        r#"[]"#,
        r#""#,
    ] {
        assert!(
            serde_json::from_str::<TakeoverLedger>(bad).is_err(),
            "{bad}"
        );
    }
}
