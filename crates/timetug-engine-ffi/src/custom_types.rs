//! The custom types of the bindings: the engine's own value types, passed across the boundary as a builtin type and shown
//! to Swift as a Foundation type by `uniffi.toml` (`Timestamp` as `Date`, `Zone` as `TimeZone`, `UrlString` as `URL`,
//! `Uuid` as `UUID`). Durations stay `f64` seconds, which Swift spells `TimeInterval`.
//!
//! A custom type that cannot be lifted is a `fatalError` in a Swift function that does not throw, and a typed error in one
//! that does (`docs/bindings-spike.md`). So a lift here fails only for a value no host can produce by accident (an unknown
//! zone identifier, text that is not a URL or a UUID), and `Timestamp` never fails: it is clamped.

use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use uuid::Uuid;

use crate::errors::FfiError;

/// `Timestamp` as milliseconds since 1970, rounded half up to the nearest millisecond (what a Swift `Date` converts to).
pub fn timestamp_to_millis(timestamp: Timestamp) -> i64 {
    let rounded = (timestamp.as_nanosecond() + 500_000).div_euclid(1_000_000);
    // `Timestamp` spans years -9999 to 9999, so the millisecond count always fits `i64`.
    i64::try_from(rounded).unwrap_or(if rounded < 0 { i64::MIN } else { i64::MAX })
}

/// Milliseconds since 1970 as a `Timestamp`; a value outside the calendar range (years -9999 to 9999) is clamped to the
/// nearest end instead of failing, so no host value can make a lift fail.
pub fn timestamp_from_millis(millis: i64) -> Timestamp {
    Timestamp::from_millisecond(millis).unwrap_or(if millis < 0 {
        Timestamp::MIN
    } else {
        Timestamp::MAX
    })
}

/// `TimeZone(identifier:)`, or an `InvalidConfig` error naming the identifier.
pub fn zone_from_identifier(identifier: &str) -> Result<Zone, FfiError> {
    Zone::named(identifier).ok_or_else(|| FfiError::InvalidConfig {
        reason: format!("unknown time zone identifier \"{identifier}\""),
    })
}

uniffi::custom_type!(Timestamp, i64, {
    remote,
    lower: |timestamp| timestamp_to_millis(timestamp),
    try_lift: |millis| Ok(timestamp_from_millis(millis)),
});

uniffi::custom_type!(Zone, String, {
    remote,
    lower: |zone| zone.identifier().to_string(),
    try_lift: |identifier| Ok(zone_from_identifier(&identifier)?),
});

uniffi::custom_type!(UrlString, String, {
    remote,
    lower: |url| url.as_str().to_string(),
    try_lift: |text| UrlString::parse(&text).ok_or_else(|| FfiError::InvalidConfig {
        reason: format!("not a URL: {text}"),
    }.into()),
});

uniffi::custom_type!(Uuid, String, {
    remote,
    // Swift's `uuidString` is upper case, and so is every identifier the engine stores.
    lower: |uuid| uuid.hyphenated().to_string().to_uppercase(),
    try_lift: |text| Uuid::parse_str(&text).map_err(|_| FfiError::InvalidConfig {
        reason: format!("not a UUID: {text}"),
    }.into()),
});
