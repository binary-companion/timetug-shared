//! Port of `EnvelopeTests.swift`. `JSONValue` is `serde_json::Value`, so its three tests check the properties the
//! protocol relies on (every shape round-trips; a boolean and a number stay distinct) on `Value`.

use serde_json::{Value, json};

use crate::config::{Invalid, WireConfig};
use crate::envelope::{Envelope, MessageId};
use crate::wire_types::WireDates;

#[test]
fn envelope_ignores_unknown_fields() {
    let json = r#"{"v":1,"type":"refresh","extra":{"a":1},"payload":null}"#;
    let e = Envelope::parse(json).unwrap();
    assert_eq!(e.r#type, "refresh");
    assert_eq!(e.v, 1);
    assert_eq!(e.payload, None);
}

#[test]
fn id_accepts_number_and_string() {
    assert_eq!(
        serde_json::from_str::<MessageId>("7").unwrap(),
        MessageId::Number(7)
    );
    assert_eq!(
        serde_json::from_str::<MessageId>(r#""cmd-7""#).unwrap(),
        MessageId::String("cmd-7".into())
    );
}

#[test]
fn id_round_trips() {
    for id in [
        MessageId::Number(18_446_744_073_709_551_615),
        MessageId::String("x".into()),
    ] {
        let text = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<MessageId>(&text).unwrap(), id);
    }
}

#[test]
fn json_value_round_trips_every_shape() {
    let value = json!({
        "n": null, "b": true, "d": 1.5, "s": "a/b",
        "a": [1, false, {}],
    });
    let text = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), value);
}

#[test]
fn boolean_and_number_stay_distinct() {
    let value: Value = serde_json::from_str("[true,1,0,false]").unwrap();
    assert_eq!(value, json!([true, 1, 0, false]));
    let items = value.as_array().unwrap();
    assert!(items[0].is_boolean() && items[1].is_number());
    assert!(items[2].is_number() && items[3].is_boolean());
}

#[test]
fn envelope_omits_absent_fields() {
    let value = Envelope::new("start").to_value();
    assert_eq!(value, json!({"v": 1, "type": "start"}));
}

#[test]
fn config_rejects_unsupported_version_and_empty_directory() {
    assert_eq!(
        WireConfig::parse(r#"{"v":2,"dataDirectory":"/x","platform":"linux"}"#),
        Err(Invalid::UnsupportedVersion(2))
    );
    assert_eq!(
        WireConfig::parse(r#"{"v":1,"dataDirectory":"","platform":"linux"}"#),
        Err(Invalid::EmptyDataDirectory)
    );
    assert_eq!(
        WireConfig::parse(r#"{"v":1,"dataDirectory":"/x","platform":"beos"}"#),
        Err(Invalid::UnknownPlatform("beos".into()))
    );
    assert_eq!(
        WireConfig::parse(
            r#"{"v":1,"dataDirectory":"/x","platform":"linux","timeZone":"Mars/Olympus"}"#
        ),
        Err(Invalid::UnknownTimeZone("Mars/Olympus".into()))
    );
    assert_eq!(WireConfig::parse("{not json"), Err(Invalid::NotJson));
}

#[test]
fn config_defaults() {
    let c = WireConfig::parse(r#"{"v":1,"dataDirectory":"/x","platform":"windows"}"#).unwrap();
    assert!(!c.testing);
    assert!(!c.text_generation);
    assert_eq!(c.time_zone, None);
    assert!(c.connectors.is_empty());
}

#[test]
fn config_reads_every_field() {
    let c = WireConfig::parse(
        r#"
        {"v":1,"dataDirectory":"/x","platform":"macos","testing":true,"textGeneration":true,
         "timeZone":"America/Chicago","connectors":{"google":{"clientID":"abc"}},"unknown":1}
        "#,
    )
    .unwrap();
    assert!(c.testing && c.text_generation);
    assert_eq!(c.time_zone.as_deref(), Some("America/Chicago"));
    assert_eq!(c.connectors["google"].client_id, "abc");
    assert_eq!(c.connectors["google"].client_secret, None);
    assert_eq!(c.test_clock_start, None);
}

#[test]
fn config_reads_the_client_secret_and_the_test_clock_start() {
    let c = WireConfig::parse(
        r#"
        {"v":1,"dataDirectory":"/x","platform":"linux","testing":true,"testClockStart":"2026-10-16T13:00:00.000Z",
         "connectors":{"google":{"clientID":"abc","clientSecret":"s"}}}
        "#,
    )
    .unwrap();
    assert_eq!(c.connectors["google"].client_secret.as_deref(), Some("s"));
    assert_eq!(
        c.test_clock_start
            .as_deref()
            .and_then(WireDates::date)
            .map(|t| t.as_second()),
        Some(1_792_155_600)
    );
    assert_eq!(
        WireConfig::parse(
            r#"{"v":1,"dataDirectory":"/x","platform":"linux","testClockStart":"yesterday"}"#
        ),
        Err(Invalid::InvalidTestClockStart("yesterday".into()))
    );
}

#[test]
fn config_without_external_tokens_decodes_as_false() {
    // The 1.7.0 config text: no `externalTokens` key at all.
    let c = WireConfig::parse(
        r#"{"v":1,"dataDirectory":"/x","platform":"windows","connectors":{"microsoft":{"clientID":"m"}}}"#,
    )
    .unwrap();
    assert!(!c.external_tokens);
}

#[test]
fn config_with_external_tokens_true_round_trips() {
    let c = WireConfig::parse(
        r#"{"v":1,"dataDirectory":"/x","platform":"windows","externalTokens":true}"#,
    )
    .unwrap();
    assert!(c.external_tokens);
    let again = WireConfig::parse(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(again, c);
    let built = WireConfig::new("/x", "windows").with_external_tokens(true);
    assert!(built.external_tokens && built == c);
}

#[test]
fn config_old_initializer_still_means_no_external_tokens() {
    assert!(!WireConfig::new("/x", "windows").external_tokens);
    let mut with_text = WireConfig::new("/x", "windows");
    with_text.text_generation = true;
    assert!(!with_text.external_tokens);
}

// MARK: - Rust-only tests

#[test]
fn envelope_reads_what_foundation_reads() {
    // A number with no fraction is an Int / UInt64 for Foundation's JSONDecoder, whatever its spelling.
    let e = Envelope::parse(r#"{"v":1.0,"type":"refresh","id":7.0}"#).unwrap();
    assert_eq!((e.v, e.id), (1, Some(MessageId::Number(7))));
    // A fraction, a negative id, a boolean or a missing member fail the whole decode.
    for bad in [
        r#"{"v":1.5,"type":"refresh"}"#,
        r#"{"v":1,"type":"refresh","id":-1}"#,
        r#"{"v":1,"type":"refresh","id":true}"#,
        r#"{"v":1,"type":"refresh","id":[]}"#,
        r#"{"v":"1","type":"refresh"}"#,
        r#"{"type":"refresh"}"#,
        r#"{"v":1}"#,
        r#"{"v":1,"type":5}"#,
        r#"[1]"#,
        "nope",
    ] {
        assert!(Envelope::parse(bad).is_err(), "{bad}");
    }
    // A null id is no id; a null payload is no payload.
    let e = Envelope::parse(r#"{"v":1,"type":"x","id":null,"payload":null}"#).unwrap();
    assert!(e.id.is_none() && e.payload.is_none());
}

#[test]
fn envelope_round_trips_through_text() {
    let e = Envelope::new("event.diagnostic")
        .with_id(MessageId::Number(u64::MAX))
        .with_payload(json!({"a": [1, 2.5, "x"]}));
    assert_eq!(Envelope::parse(&e.to_json()).unwrap(), e);
    assert_eq!(serde_json::to_value(&e).unwrap(), e.to_value());
    assert_eq!(serde_json::from_str::<Envelope>(&e.to_json()).unwrap(), e);
}

#[test]
fn config_reads_a_whole_number_version_and_null_members() {
    let c = WireConfig::parse(
        r#"{"v":1.0,"dataDirectory":"/x","platform":"linux","connectors":null,"testing":null,"timeZone":null}"#,
    )
    .unwrap();
    assert!(!c.testing && c.time_zone.is_none() && c.connectors.is_empty());
    // A wrongly typed member fails the whole decode.
    for bad in [
        r#"{"v":1,"dataDirectory":"/x","platform":"linux","testing":"yes"}"#,
        r#"{"v":1,"dataDirectory":7,"platform":"linux"}"#,
        r#"{"v":1,"dataDirectory":"/x","platform":"linux","connectors":{"google":{}}}"#,
        r#"{"v":null,"dataDirectory":"/x","platform":"linux"}"#,
        r#"{"dataDirectory":"/x","platform":"linux"}"#,
        "[]",
        "",
    ] {
        assert_eq!(WireConfig::parse(bad), Err(Invalid::NotJson), "{bad}");
    }
}

#[test]
fn config_validation_runs_in_swift_order() {
    // Everything wrong at once: the version is reported first, then the directory, platform, zone and clock start.
    let all_wrong = |v: i64, dir: &str, platform: &str, zone: &str, start: &str| {
        WireConfig::parse(&format!(
            r#"{{"v":{v},"dataDirectory":"{dir}","platform":"{platform}","timeZone":"{zone}","testClockStart":"{start}"}}"#
        ))
    };
    assert_eq!(
        all_wrong(2, "", "beos", "Mars/X", "x"),
        Err(Invalid::UnsupportedVersion(2))
    );
    assert_eq!(
        all_wrong(1, "", "beos", "Mars/X", "x"),
        Err(Invalid::EmptyDataDirectory)
    );
    assert_eq!(
        all_wrong(1, "/x", "beos", "Mars/X", "x"),
        Err(Invalid::UnknownPlatform("beos".into()))
    );
    assert_eq!(
        all_wrong(1, "/x", "linux", "Mars/X", "x"),
        Err(Invalid::UnknownTimeZone("Mars/X".into()))
    );
    assert_eq!(
        all_wrong(1, "/x", "linux", "Europe/Berlin", "x"),
        Err(Invalid::InvalidTestClockStart("x".into()))
    );
    for platform in ["windows", "linux", "macos"] {
        assert!(all_wrong(1, "/x", platform, "Europe/Berlin", "2026-10-16T13:00:00Z").is_ok());
    }
}
