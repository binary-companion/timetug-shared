//! Port of `WireTypes.swift`: the wire dates and the event, state and settings DTOs.
//!
//! Every DTO names its JSON members explicitly (the Swift property names, `sourceID` and not `sourceId`). An optional
//! member that is `nil` in Swift is omitted on write, as Swift's synthesized `encodeIfPresent` does; the fixtures pin it.
//! Every Swift `Double` is written through `calendar_core::json::f64_swift`, so `120.0` is written `120`.

use std::collections::BTreeMap;

use calendar_core::json::{f64_swift, f64_swift_option};
use jiff::Timestamp;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize, Serializer};

/// A `Vec<f64>` whose elements are written through [`f64_swift`] (`[60, 300, 600]`, not `[60.0, ...]`).
fn f64_swift_vec<S: Serializer>(values: &[f64], serializer: S) -> Result<S::Ok, S::Error> {
    struct One<'a>(&'a f64);
    impl Serialize for One<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            f64_swift(self.0, serializer)
        }
    }
    let mut seq = serializer.serialize_seq(Some(values.len()))?;
    for value in values {
        seq.serialize_element(&One(value))?;
    }
    seq.end()
}

/// Seconds from 1970-01-01 to 2001-01-01, the zero of Foundation's `Date`.
const REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

/// Instants and calendar dates as the wire writes them. Hand-formatted (no `ISO8601DateFormatter`, whose options differ
/// between Darwin and swift-corelibs-foundation) so Mac, Linux and Windows emit the same bytes.
pub enum WireDates {}

impl WireDates {
    /// `2025-10-09T08:53:20.123Z`: UTC, millisecond precision (rounded to the nearest millisecond, halves away from
    /// zero). The rounding is done on the nanoseconds of the `Timestamp`, so it is exact.
    pub fn string(date: Timestamp) -> String {
        let nanos = date.as_nanosecond();
        let millis = if nanos >= 0 {
            (nanos + 500_000) / 1_000_000
        } else {
            -((-nanos + 500_000) / 1_000_000)
        };
        // A `Timestamp` spans about 6.2e14 milliseconds either way, far inside `i64`.
        Self::string_from_millis(millis as i64)
    }

    /// Swift's `string(Date(timeIntervalSince1970: seconds))`: the `Date` is a `Double` of seconds since 2001, so the
    /// seconds first make the round trip through that reference (`vectors/wire/dates.json` is produced that way).
    /// Values beyond year 99999 are clamped; a `Timestamp` never reaches them.
    pub fn string_from_unix_seconds(seconds: f64) -> String {
        let stored = seconds - REFERENCE_DATE_OFFSET;
        let back = stored + REFERENCE_DATE_OFFSET;
        let millis = (back * 1000.0).round().clamp(-1.0e17, 1.0e17);
        Self::string_from_millis(millis as i64)
    }

    fn string_from_millis(total_millis: i64) -> String {
        let seconds = floor_div(total_millis, 1000);
        let millis = total_millis - seconds * 1000;
        let days = floor_div(seconds, 86_400);
        let second_of_day = seconds - days * 86_400;
        let (y, m, d) = civil_from_days(days);
        format!(
            "{}-{}-{}T{}:{}:{}.{}Z",
            pad(y, 4),
            pad(m, 2),
            pad(d, 2),
            pad(second_of_day / 3600, 2),
            pad(second_of_day / 60 % 60, 2),
            pad(second_of_day % 60, 2),
            pad(millis, 3)
        )
    }

    /// Reads the format `string` writes (fraction optional). `None` for anything else, and for an instant a `Timestamp`
    /// cannot hold (the last 26 hours of year 9999; see [`WireDates::unix_nanoseconds`]).
    pub fn date(text: &str) -> Option<Timestamp> {
        Timestamp::from_nanosecond(Self::unix_nanoseconds(text)?).ok()
    }

    /// The text as nanoseconds since 1970-01-01T00:00:00Z, or `None` when it is not what `string` writes. This is the
    /// parser itself; `date` is its `Timestamp` form. `WireConfig::parse` validates `testClockStart` with this one, as
    /// Swift's `Date` holds every instant the parser accepts.
    pub fn unix_nanoseconds(text: &str) -> Option<i128> {
        let scalars = text.as_bytes();
        let num = |from: usize, to: usize| -> Option<i64> {
            if to > scalars.len() {
                return None;
            }
            let mut value: i64 = 0;
            for &byte in &scalars[from..to] {
                if !byte.is_ascii_digit() {
                    return None;
                }
                value = value * 10 + i64::from(byte - 48);
            }
            Some(value)
        };
        if scalars.len() < 20
            || scalars[4] != b'-'
            || scalars[7] != b'-'
            || scalars[10] != b'T'
            || scalars[13] != b':'
            || scalars[16] != b':'
            || scalars.last() != Some(&b'Z')
        {
            return None;
        }
        let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
        let (h, mi, s) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
        if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h >= 24 || mi >= 60 || s >= 61 {
            return None;
        }
        let mut fraction_nanos: i64 = 0;
        if scalars.len() > 20 {
            // At most nine fraction digits: more cannot matter for a Date and would overflow `num`.
            if scalars[19] != b'.' || scalars.len() <= 21 || scalars.len() > 30 {
                return None;
            }
            let digits = scalars.len() - 21;
            let f = num(20, scalars.len() - 1)?;
            fraction_nanos = f * 10_i64.pow(9 - digits as u32);
        } else if scalars[19] != b'Z' {
            return None;
        }
        let days = days_from_civil(y, mo, d);
        // Reject impossible dates ("2025-02-30"): the round trip must give back the same day.
        if civil_from_days(days) != (y, mo, d) {
            return None;
        }
        let seconds = days * 86_400 + (h * 3600 + mi * 60 + s);
        Some(i128::from(seconds) * 1_000_000_000 + i128::from(fraction_nanos))
    }

    /// `YYYY-MM-DD`.
    pub fn date_string(year: i64, month: i64, day: i64) -> String {
        format!("{}-{}-{}", pad(year, 4), pad(month, 2), pad(day, 2))
    }
}

/// `String(value)` left-padded with zeros to `width` (a negative number keeps its sign inside the padding, as Swift's
/// does: `-5` at width 4 is `00-5`).
fn pad(value: i64, width: usize) -> String {
    let s = value.to_string();
    let fill = width.saturating_sub(s.chars().count());
    format!("{}{s}", "0".repeat(fill))
}

fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b != 0 && (a < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

// Howard Hinnant's civil-date algorithms (proleptic Gregorian, days since 1970-01-01).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = floor_div(z, 146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let yy = if m <= 2 { y - 1 } else { y };
    let era = floor_div(yy, 400);
    let yoe = yy - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// MARK: - Event DTOs

/// A merged event's original copy. `memberID` is the copy's `contentKey` (title, start and end, lowercased and
/// second-resolution): the engine keeps no per-copy event id, and `dedup.separate` resolves its `memberIDs` against the
/// merged event's own member list by exactly this key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireMember {
    #[serde(rename = "memberID")]
    pub member_id: String,
    #[serde(rename = "calendarKey")]
    pub calendar_key: String,
    pub start: String,
    pub end: String,
    pub details: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireConference {
    pub url: String,
    /// `meet`, `teams`, `zoom`, `webex`, `goToMeeting`, `whereby`, `jitsi`, `slack` or `other`.
    pub provider: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireProvenance {
    /// `rule`, `inference` or `userConfirmed`.
    pub kind: String,
    #[serde(default, rename = "engineID", skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<String>,
    #[serde(
        default,
        rename = "engineName",
        skip_serializing_if = "Option::is_none"
    )]
    pub engine_name: Option<String>,
}

/// One event. A timed event carries `start` and `end` instants; an all-day event carries `startDate` and `endDate`
/// (exclusive) read in `timeZone`, and no instants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEvent {
    pub id: String,
    #[serde(rename = "sourceID")]
    pub source_id: String,
    #[serde(rename = "sourceEventID")]
    pub source_event_id: String,
    #[serde(rename = "calendarKey")]
    pub calendar_key: String,
    pub title: String,
    #[serde(rename = "isAllDay")]
    pub is_all_day: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default, rename = "startDate", skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, rename = "endDate", skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// Where the shown range starts when it differs from `start` (a merged meeting); timed events only.
    #[serde(
        default,
        rename = "displayStart",
        skip_serializing_if = "Option::is_none"
    )]
    pub display_start: Option<String>,
    #[serde(rename = "timeZone")]
    pub time_zone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// `confirmed`, `tentative` or `cancelled`.
    pub status: String,
    pub conferences: Vec<WireConference>,
    #[serde(rename = "otherAttendeeCount")]
    pub other_attendee_count: i64,
    /// `accepted`, `tentative`, `declined` or `needsAction`; absent when the provider does not say.
    #[serde(
        default,
        rename = "responseStatus",
        skip_serializing_if = "Option::is_none"
    )]
    pub response_status: Option<String>,
    #[serde(rename = "additionalCalendarKeys")]
    pub additional_calendar_keys: Vec<String>,
    pub members: Vec<WireMember>,
    #[serde(
        default,
        rename = "mergeProvenance",
        skip_serializing_if = "Option::is_none"
    )]
    pub merge_provenance: Option<WireProvenance>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAgendaItem {
    #[serde(rename = "eventID")]
    pub event_id: String,
    /// `past`, `current` or `upcoming`.
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAgenda {
    pub items: Vec<WireAgendaItem>,
    #[serde(
        default,
        rename = "nextEventID",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_event_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCalendar {
    pub key: String,
    #[serde(rename = "sourceID")]
    pub source_id: String,
    #[serde(rename = "calendarID")]
    pub calendar_id: String,
    pub title: String,
    #[serde(
        default,
        rename = "accountName",
        skip_serializing_if = "Option::is_none"
    )]
    pub account_name: Option<String>,
    #[serde(default, rename = "colorHex", skip_serializing_if = "Option::is_none")]
    pub color_hex: Option<String>,
    /// `standard`, `subscribed` or `birthdays`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSourceStatus {
    /// `ok`, `needsPermission`, `authExpired` or `failing`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireNotice {
    pub kind: String,
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAccount {
    #[serde(rename = "kindID")]
    pub kind_id: String,
    #[serde(rename = "connectionID")]
    pub connection_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAccountStatus {
    #[serde(rename = "isWorking")]
    pub is_working: bool,
    #[serde(rename = "buildFailures")]
    pub build_failures: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "errorMessage",
        skip_serializing_if = "Option::is_none"
    )]
    pub error_message: Option<String>,
    /// `browser` or `inApp`.
    #[serde(rename = "waitingStyle")]
    pub waiting_style: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAccountKind {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    /// `oauth`, `password` or `system`.
    pub authorization: String,
    #[serde(rename = "isConfigured")]
    pub is_configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireInferenceStatus {
    /// `disabled`, `noEngine`, `unavailable`, `notOnDevice` or `active`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, rename = "engineID", skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<String>,
    #[serde(
        default,
        rename = "engineName",
        skip_serializing_if = "Option::is_none"
    )]
    pub engine_name: Option<String>,
    #[serde(
        default,
        rename = "isOnDevice",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_on_device: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireLedger {
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireNextTakeover {
    #[serde(rename = "eventID")]
    pub event_id: String,
    #[serde(rename = "fireAt")]
    pub fire_at: String,
}

// MARK: - Settings

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireTakeoverSettings {
    #[serde(rename = "leadTime", serialize_with = "f64_swift")]
    pub lead_time: f64,
    #[serde(rename = "takeoverCalendarKeys")]
    pub takeover_calendar_keys: Vec<String>,
    #[serde(rename = "hiddenCalendarKeys")]
    pub hidden_calendar_keys: Vec<String>,
    #[serde(rename = "requireConferenceLink")]
    pub require_conference_link: bool,
    #[serde(rename = "requireOtherAttendees")]
    pub require_other_attendees: bool,
    #[serde(rename = "skipAllDayEvents")]
    pub skip_all_day_events: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireSettings {
    pub takeover: WireTakeoverSettings,
    #[serde(rename = "inferenceEnabled")]
    pub inference_enabled: bool,
    #[serde(rename = "systemCalendarEnabled")]
    pub system_calendar_enabled: bool,
}

/// `settings.apply` payload: every field optional, absent means unchanged. The takeover fields sit flat beside the two
/// top-level switches; a calendar key list replaces the stored list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WireSettingsPatch {
    #[serde(
        default,
        rename = "leadTime",
        serialize_with = "f64_swift_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub lead_time: Option<f64>,
    #[serde(
        default,
        rename = "takeoverCalendarKeys",
        skip_serializing_if = "Option::is_none"
    )]
    pub takeover_calendar_keys: Option<Vec<String>>,
    #[serde(
        default,
        rename = "hiddenCalendarKeys",
        skip_serializing_if = "Option::is_none"
    )]
    pub hidden_calendar_keys: Option<Vec<String>>,
    #[serde(
        default,
        rename = "requireConferenceLink",
        skip_serializing_if = "Option::is_none"
    )]
    pub require_conference_link: Option<bool>,
    #[serde(
        default,
        rename = "requireOtherAttendees",
        skip_serializing_if = "Option::is_none"
    )]
    pub require_other_attendees: Option<bool>,
    #[serde(
        default,
        rename = "skipAllDayEvents",
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_all_day_events: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(
        default,
        rename = "inferenceEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub inference_enabled: Option<bool>,
    #[serde(
        default,
        rename = "systemCalendarEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub system_calendar_enabled: Option<bool>,
}

// MARK: - State and events

/// The whole `EngineState`, field for field. `FIELD_NAMES` is the coding-key list itself, so a test can compare it with
/// the engine type's fields and a later engine field cannot be forgotten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireState {
    pub revision: u64,
    pub agenda: WireAgenda,
    pub events: Vec<WireEvent>,
    pub calendars: Vec<WireCalendar>,
    pub statuses: BTreeMap<String, WireSourceStatus>,
    pub notices: BTreeMap<String, Vec<WireNotice>>,
    #[serde(rename = "sourceNames")]
    pub source_names: BTreeMap<String, String>,
    /// Pending duplicate pairs: an event id to the ids of the events it may duplicate; all resolve in `events`.
    pub candidates: BTreeMap<String, Vec<String>>,
    pub accounts: Vec<WireAccount>,
    #[serde(rename = "accountStatus")]
    pub account_status: WireAccountStatus,
    pub settings: WireSettings,
    #[serde(rename = "inferenceStatus")]
    pub inference_status: WireInferenceStatus,
    pub ledger: WireLedger,
    #[serde(
        default,
        rename = "nextTakeover",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_takeover: Option<WireNextTakeover>,
    #[serde(rename = "takeoverVisible")]
    pub takeover_visible: bool,
    #[serde(rename = "availableAccountKinds")]
    pub available_account_kinds: Vec<WireAccountKind>,
}

impl WireState {
    /// Swift's `CodingKeys.allCases` raw values, in declaration order.
    pub const FIELD_NAMES: [&'static str; 16] = [
        "revision",
        "agenda",
        "events",
        "calendars",
        "statuses",
        "notices",
        "sourceNames",
        "candidates",
        "accounts",
        "accountStatus",
        "settings",
        "inferenceStatus",
        "ledger",
        "nextTakeover",
        "takeoverVisible",
        "availableAccountKinds",
    ];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireTakeoverRequest {
    pub event: WireEvent,
    #[serde(rename = "hasStarted")]
    pub has_started: bool,
    #[serde(default, rename = "joinURL", skip_serializing_if = "Option::is_none")]
    pub join_url: Option<String>,
    /// Seconds.
    #[serde(rename = "snoozeOptions", serialize_with = "f64_swift_vec")]
    pub snooze_options: Vec<f64>,
    #[serde(
        default,
        rename = "calendarTitle",
        skip_serializing_if = "Option::is_none"
    )]
    pub calendar_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireWidgetEvent {
    pub id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    #[serde(rename = "isAllDay")]
    pub is_all_day: bool,
    #[serde(default, rename = "colorHex", skip_serializing_if = "Option::is_none")]
    pub color_hex: Option<String>,
    #[serde(default, rename = "joinURL", skip_serializing_if = "Option::is_none")]
    pub join_url: Option<String>,
}

/// A view model for widgets: all-day events carry the local midnights of their dates, as instants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireWidgetSnapshot {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub events: Vec<WireWidgetEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDiagnostic {
    /// `info`, `warning` or `error`.
    pub level: String,
    pub category: String,
    pub message: String,
}
