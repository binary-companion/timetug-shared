//! Parsers never panic (conventions section 1): hostile or damaged settings text is an error or a value, nothing else.

use crate::model::takeover_settings::TakeoverSettings;

#[test]
fn settings_decoding_survives_damaged_input() {
    let inputs: Vec<String> = vec![
        String::new(),
        "null".into(),
        "[]".into(),
        "[1,2,3]".into(),
        "\"text\"".into(),
        "123".into(),
        "{".into(),
        r#"{"leadTime":"soon"}"#.into(),
        r#"{"leadTime":1e999}"#.into(),
        r#"{"leadTime":-1e999}"#.into(),
        r#"{"leadTime":NaN}"#.into(),
        r#"{"takeoverCalendarKeys":"a/b"}"#.into(),
        r#"{"takeoverCalendarKeys":[1,2]}"#.into(),
        r#"{"takeoverCalendarKeys":[null]}"#.into(),
        r#"{"hiddenCalendarKeys":{"a":1}}"#.into(),
        r#"{"enabled":"yes"}"#.into(),
        r#"{"enabled":1}"#.into(),
        r#"{"leadTime":1,"leadTime":2}"#.into(),
        "{\"takeoverCalendarKeys\":[\"\u{0}\",\"\u{301}/\u{301}\",\"\"]}".into(),
        format!("{}1{}", "[".repeat(10_000), "]".repeat(10_000)),
        format!(
            r#"{{"takeoverCalendarKeys":[{}]}}"#,
            "\"k\",".repeat(50_000) + "\"k\""
        ),
    ];
    for input in &inputs {
        let short: String = input.chars().take(40).collect();
        let _ = serde_json::from_str::<TakeoverSettings>(input).map_err(|e| e.to_string());
        let _ = short;
    }
    // Strict readers that reject a shape still say so.
    assert!(serde_json::from_str::<TakeoverSettings>("[]").is_err());
    assert!(serde_json::from_str::<TakeoverSettings>(r#"{"leadTime":"soon"}"#).is_err());
    // The last of two equal keys wins; the file stays readable.
    assert_eq!(
        serde_json::from_str::<TakeoverSettings>(r#"{"leadTime":1,"leadTime":2}"#)
            .unwrap()
            .lead_time,
        2.0
    );
}

#[test]
fn removing_calendars_handles_keys_without_a_slash_or_empty_keys() {
    let mut s = TakeoverSettings::default();
    s.takeover_calendar_keys = ["", "/", "plain", "a//b"]
        .iter()
        .map(|k| k.to_string())
        .collect();
    s.remove_calendars_where_source_id(|id| id.is_empty());
    assert_eq!(
        s.takeover_calendar_keys,
        ["plain", "a//b"].iter().map(|k| k.to_string()).collect()
    );
    s.remove_calendars_for_source_id("plain");
    assert_eq!(
        s.takeover_calendar_keys,
        ["a//b"].iter().map(|k| k.to_string()).collect()
    );
}
