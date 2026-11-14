//! Round trips of the custom types over the UniFFI bridge types (`i64`, `String`, `f64`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use uuid::Uuid;

use crate::UniFfiTag;
use crate::custom_types::{timestamp_from_millis, timestamp_to_millis, zone_from_identifier};
use crate::errors::FfiError;

/// Lowers `value` to its bridge type and lifts it back, as a host call and its reply do.
fn over_the_bridge<T: uniffi::FfiConverter<UniFfiTag>>(value: T) -> T {
    let lowered = <T as uniffi::FfiConverter<UniFfiTag>>::lower(value);
    <T as uniffi::FfiConverter<UniFfiTag>>::try_lift(lowered).unwrap()
}

#[test]
fn timestamp_with_sub_millisecond_precision_rounds_to_the_nearest_millisecond() {
    // (nanoseconds since 1970, expected milliseconds), table-driven, both sides of the half.
    let cases: [(i128, i64); 8] = [
        (1_700_000_000_000_000_000, 1_700_000_000_000),
        (1_700_000_000_000_400_000, 1_700_000_000_000),
        (1_700_000_000_000_500_000, 1_700_000_000_001),
        (1_700_000_000_000_600_000, 1_700_000_000_001),
        (0, 0),
        (499_999, 0),
        (-500_001, -1),
        (-499_999, 0),
    ];
    for (nanoseconds, expected) in cases {
        let timestamp = Timestamp::from_nanosecond(nanoseconds).unwrap();
        assert_eq!(timestamp_to_millis(timestamp), expected, "{nanoseconds} ns");
    }
}

#[test]
fn timestamp_round_trips_at_millisecond_precision() {
    for millis in [
        0_i64,
        1,
        -1,
        1_700_000_000_123,
        -62_135_596_800_000,
        253_402_207_199_999,
    ] {
        let timestamp = timestamp_from_millis(millis);
        assert_eq!(timestamp.as_millisecond(), millis);
        assert_eq!(over_the_bridge(timestamp).as_millisecond(), millis);
    }
}

#[test]
fn timestamp_beyond_the_calendar_range_is_clamped_not_rejected() {
    assert_eq!(timestamp_from_millis(i64::MAX), Timestamp::MAX);
    assert_eq!(timestamp_from_millis(i64::MIN), Timestamp::MIN);
}

#[test]
fn zone_round_trips_named_and_fixed_offset_zones() {
    for identifier in [
        "Europe/Berlin",
        "America/New_York",
        "GMT",
        "GMT+0100",
        "GMT-0530",
        "GMT+0000",
    ] {
        let zone = Zone::named(identifier).unwrap();
        let back = over_the_bridge(zone.clone());
        assert_eq!(back, zone, "{identifier}");
        assert_eq!(back.identifier(), zone.identifier(), "{identifier}");
    }
}

#[test]
fn a_fixed_offset_zone_uses_the_identifier_foundation_reports() {
    let zone = Zone::fixed(3600).unwrap();
    assert_eq!(zone.identifier(), "GMT+0100");
    assert_eq!(zone_from_identifier("GMT+0100").unwrap(), zone);
    // Foundation also reads the short custom forms.
    assert_eq!(zone_from_identifier("GMT+1").unwrap(), zone);
}

#[test]
fn an_unknown_zone_identifier_is_an_error_not_a_panic() {
    for identifier in [
        "",
        "Not/AZone",
        "europe/berlin",
        "Europe/Berlin ",
        "GMT+9999",
    ] {
        let error = zone_from_identifier(identifier).unwrap_err();
        assert_eq!(
            error,
            FfiError::InvalidConfig {
                reason: format!("unknown time zone identifier \"{identifier}\"")
            },
            "{identifier:?}"
        );
    }
}

#[test]
fn a_zone_the_host_sends_that_is_unknown_fails_the_lift() {
    let lowered = <String as uniffi::Lower<UniFfiTag>>::lower("Not/AZone".to_string());
    assert!(<Zone as uniffi::Lift<UniFfiTag>>::try_lift(lowered).is_err());
}

#[test]
fn url_string_keeps_its_original_text_including_non_ascii() {
    for text in [
        "https://example.com/",
        "https://example.com/caf\u{e9}/\u{65e5}\u{672c}?q=a b#frag",
        "mailto:someone@example.com",
        "HTTPS://Example.COM/Path",
    ] {
        let url = UrlString::parse(text).unwrap();
        let back = over_the_bridge(url);
        assert_eq!(back.as_str(), text);
    }
}

#[test]
fn a_url_the_host_sends_that_foundation_would_refuse_fails_the_lift() {
    let lowered = <String as uniffi::Lower<UniFfiTag>>::lower("http://exa mple.com/".to_string());
    assert!(<UrlString as uniffi::Lift<UniFfiTag>>::try_lift(lowered).is_err());
}

#[test]
fn uuid_crosses_as_upper_case_text_and_is_read_in_either_case() {
    let uuid = Uuid::parse_str("0f8fad5b-d9cb-469f-a165-70867728950e").unwrap();
    assert_eq!(over_the_bridge(uuid), uuid);
    let lowered = <Uuid as uniffi::Lower<UniFfiTag>>::lower(uuid);
    let text = <String as uniffi::Lift<UniFfiTag>>::try_lift(lowered).unwrap();
    assert_eq!(text, "0F8FAD5B-D9CB-469F-A165-70867728950E");
    for spelled in [text.clone(), text.to_lowercase()] {
        let lowered = <String as uniffi::Lower<UniFfiTag>>::lower(spelled);
        assert_eq!(
            <Uuid as uniffi::Lift<UniFfiTag>>::try_lift(lowered).unwrap(),
            uuid
        );
    }
    let lowered = <String as uniffi::Lower<UniFfiTag>>::lower("not-a-uuid".to_string());
    assert!(<Uuid as uniffi::Lift<UniFfiTag>>::try_lift(lowered).is_err());
}
