//! Replays `vectors/text/dedup-key.json`: `TimeTugCalendarEvent.contentKey` for 101 titles and times, made by the
//! Swift code at the baseline. The cases pin `String.lowercased()` (no final-sigma rule, `İ` becomes `i` plus a
//! combining dot) and `Int(timeIntervalSince1970)`.

use calendar_core::model::CalendarEvent;
use jiff::Timestamp;
use serde_json::Value;

use crate::model::event::TimeTugCalendarEvent;

const DEDUP_KEY: &str = include_str!("../../../../vectors/text/dedup-key.json");

#[test]
fn dedup_key_vector() {
    let file: Value = serde_json::from_str(DEDUP_KEY).unwrap();
    assert_eq!(file["area"], "text/dedup-key");
    let cases = file["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let start = Timestamp::from_millisecond(input["start"].as_i64().unwrap()).unwrap();
        let end = Timestamp::from_millisecond(input["end"].as_i64().unwrap()).unwrap();
        let title = input["title"].as_str().unwrap();
        let event =
            TimeTugCalendarEvent::new(CalendarEvent::new("e", "cal", title, start, end), "src");
        let expected = case["expected"]["key"].as_str().unwrap();
        let actual = event.content_key();
        if actual != expected {
            failures.push(format!(
                "{name}: title {title:?}: expected {expected:?}, got {actual:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "text/dedup-key: {} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn id_and_content_key_truncate_toward_zero_before_1970() {
    // Swift: `Int(Date(timeIntervalSince1970: -0.5).timeIntervalSince1970)` is 0, not -1.
    let start = Timestamp::from_millisecond(-500).unwrap();
    let end = Timestamp::from_millisecond(1_500).unwrap();
    let event =
        TimeTugCalendarEvent::new(CalendarEvent::new("e1", "cal", "Old", start, end), "src");
    assert_eq!(event.id(), "src/e1/0");
    assert_eq!(event.content_key(), "old|0|1");
    let earlier = Timestamp::from_millisecond(-1_500).unwrap();
    let event =
        TimeTugCalendarEvent::new(CalendarEvent::new("e1", "cal", "Old", earlier, end), "src");
    assert_eq!(event.id(), "src/e1/-1");
}
