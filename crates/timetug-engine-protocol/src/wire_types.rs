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
    /// The copy's `contentKey`.
    pub member_id: String,
    #[serde(rename = "calendarKey")]
    /// Key of the calendar the copy came from.
    pub calendar_key: String,
    /// Start of the copy, `WireDates` format.
    pub start: String,
    /// End of the copy, `WireDates` format.
    pub end: String,
    /// Which details the copy had, for example `bare` or `location+notes`.
    pub details: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// A video-meeting link found on an event.
pub struct WireConference {
    /// The meeting URL.
    pub url: String,
    /// `meet`, `teams`, `zoom`, `webex`, `goToMeeting`, `whereby`, `jitsi`, `slack` or `other`.
    pub provider: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// How a merge was decided.
pub struct WireProvenance {
    /// `rule`, `inference` or `userConfirmed`.
    pub kind: String,
    #[serde(default, rename = "engineID", skip_serializing_if = "Option::is_none")]
    /// Id of the text model, for kind `inference`.
    pub engine_id: Option<String>,
    #[serde(
        default,
        rename = "engineName",
        skip_serializing_if = "Option::is_none"
    )]
    /// Display name of the text model, for kind `inference`.
    pub engine_name: Option<String>,
}

/// One event. A timed event carries `start` and `end` instants; an all-day event carries `startDate` and `endDate`
/// (exclusive) read in `timeZone`, and no instants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEvent {
    /// The engine's id for the event; commands such as `takeover.join` refer to events by it.
    pub id: String,
    #[serde(rename = "sourceID")]
    /// Id of the calendar source (an account or the system calendar) the event came from.
    pub source_id: String,
    #[serde(rename = "sourceEventID")]
    /// The event's id within its source.
    pub source_event_id: String,
    #[serde(rename = "calendarKey")]
    /// Key of the event's calendar, `sourceID/calendarID`; matches `WireCalendar::key`.
    pub calendar_key: String,
    /// Event title.
    pub title: String,
    #[serde(rename = "isAllDay")]
    /// True for an all-day event, which carries `startDate` and `endDate` instead of `start` and `end`.
    pub is_all_day: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Start instant, `WireDates` format; timed events only.
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// End instant, `WireDates` format; timed events only.
    pub end: Option<String>,
    #[serde(default, rename = "startDate", skip_serializing_if = "Option::is_none")]
    /// First day, `YYYY-MM-DD`; all-day events only.
    pub start_date: Option<String>,
    #[serde(default, rename = "endDate", skip_serializing_if = "Option::is_none")]
    /// Day after the last day (exclusive), `YYYY-MM-DD`; all-day events only.
    pub end_date: Option<String>,
    /// Where the shown range starts when it differs from `start` (a merged meeting); timed events only.
    #[serde(
        default,
        rename = "displayStart",
        skip_serializing_if = "Option::is_none"
    )]
    pub display_start: Option<String>,
    #[serde(rename = "timeZone")]
    /// IANA id of the zone the event is read in.
    pub time_zone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Where the event takes place, if the source has it.
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The event's own web page, if the source has one.
    pub url: Option<String>,
    /// `confirmed`, `tentative` or `cancelled`.
    pub status: String,
    /// Video-meeting links found on the event.
    pub conferences: Vec<WireConference>,
    #[serde(rename = "otherAttendeeCount")]
    /// Number of attendees other than the person.
    pub other_attendee_count: i64,
    /// `accepted`, `tentative`, `declined` or `needsAction`; absent when the provider does not say.
    #[serde(
        default,
        rename = "responseStatus",
        skip_serializing_if = "Option::is_none"
    )]
    pub response_status: Option<String>,
    #[serde(rename = "additionalCalendarKeys")]
    /// Keys of the other calendars that hold a copy of the event, sorted.
    pub additional_calendar_keys: Vec<String>,
    /// The original copies, when the event is a merge; empty otherwise.
    pub members: Vec<WireMember>,
    #[serde(
        default,
        rename = "mergeProvenance",
        skip_serializing_if = "Option::is_none"
    )]
    /// How the merge was decided; absent when the event is not a merge.
    pub merge_provenance: Option<WireProvenance>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// One agenda entry.
pub struct WireAgendaItem {
    #[serde(rename = "eventID")]
    /// Id of the event, which resolves in `WireState::events`.
    pub event_id: String,
    /// `past`, `current` or `upcoming`.
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// The day's agenda.
pub struct WireAgenda {
    /// The agenda entries.
    pub items: Vec<WireAgendaItem>,
    #[serde(
        default,
        rename = "nextEventID",
        skip_serializing_if = "Option::is_none"
    )]
    /// Id of the next upcoming event, if there is one.
    pub next_event_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// A calendar a source offers.
pub struct WireCalendar {
    /// Calendar key, `sourceID/calendarID`; the form used in the settings key lists and in `WireEvent::calendar_key`.
    pub key: String,
    #[serde(rename = "sourceID")]
    /// Id of the calendar source (an account or the system calendar).
    pub source_id: String,
    #[serde(rename = "calendarID")]
    /// The calendar's id within its source.
    pub calendar_id: String,
    /// Calendar title.
    pub title: String,
    #[serde(
        default,
        rename = "accountName",
        skip_serializing_if = "Option::is_none"
    )]
    /// Name of the account the calendar belongs to, if any.
    pub account_name: Option<String>,
    #[serde(default, rename = "colorHex", skip_serializing_if = "Option::is_none")]
    /// Calendar color as `#RRGGBB`, if the source has one.
    pub color_hex: Option<String>,
    /// `standard`, `subscribed` or `birthdays`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The service the calendar belongs to, for example `google`; absent when unknown.
    pub service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The provider of the calendar, for example `google`; absent when unknown.
    pub provider: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Health of one source.
pub struct WireSourceStatus {
    /// `ok`, `needsPermission`, `authExpired` or `failing`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Failure text, with kind `failing`.
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// A quiet hint about a source, shown as a warning icon.
pub struct WireNotice {
    /// What the hint is about; currently `unreadableRecurrence` (repeating events whose rule could not be read).
    pub kind: String,
    /// Number of items the hint covers.
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// A stored account.
pub struct WireAccount {
    #[serde(rename = "kindID")]
    /// Id of the connector kind, as in `WireAccountKind::id`.
    pub kind_id: String,
    #[serde(rename = "connectionID")]
    /// Id of the stored connection; used by `account.reconnect` and `account.remove`.
    pub connection_id: String,
    #[serde(rename = "displayName")]
    /// Name to show for the account.
    pub display_name: String,
    /// Non-secret settings of the connection, by name (for example a server URL).
    pub config: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Progress and errors of account sign-in.
pub struct WireAccountStatus {
    #[serde(rename = "isWorking")]
    /// True while a sign-in, re-authorization or removal is in progress.
    pub is_working: bool,
    #[serde(rename = "buildFailures")]
    /// Reason the source could not be built, by connection id.
    pub build_failures: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "errorMessage",
        skip_serializing_if = "Option::is_none"
    )]
    /// Text of the last failure, if any.
    pub error_message: Option<String>,
    /// `browser` or `inApp`.
    #[serde(rename = "waitingStyle")]
    pub waiting_style: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// A kind of account the person can add.
pub struct WireAccountKind {
    /// Connector kind id, as passed in `account.add`.
    pub id: String,
    #[serde(rename = "displayName")]
    /// Name to show for the kind.
    pub display_name: String,
    /// `oauth`, `password` or `system`.
    pub authorization: String,
    #[serde(rename = "isConfigured")]
    /// False when the platform supports the kind but this build cannot use it (for example Google without an OAuth
    /// client).
    pub is_configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Whether the text model is in use.
pub struct WireInferenceStatus {
    /// `disabled`, `noEngine`, `unavailable`, `notOnDevice` or `active`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Why the model is unavailable, with kind `unavailable`.
    pub reason: Option<String>,
    #[serde(default, rename = "engineID", skip_serializing_if = "Option::is_none")]
    /// Id of the model, with kind `active`.
    pub engine_id: Option<String>,
    #[serde(
        default,
        rename = "engineName",
        skip_serializing_if = "Option::is_none"
    )]
    /// Display name of the model, with kind `active`.
    pub engine_name: Option<String>,
    #[serde(
        default,
        rename = "isOnDevice",
        skip_serializing_if = "Option::is_none"
    )]
    /// True when the model runs on the device, with kind `active`.
    pub is_on_device: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Summary of the takeover ledger.
pub struct WireLedger {
    /// Number of keys in the ledger (takeovers already fired, snoozed or dismissed).
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// The takeover the engine has armed.
pub struct WireNextTakeover {
    #[serde(rename = "eventID")]
    /// Id of the event the takeover is for.
    pub event_id: String,
    #[serde(rename = "fireAt")]
    /// When the takeover fires, `WireDates` format.
    pub fire_at: String,
}

// MARK: - Settings

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// The takeover part of the settings.
pub struct WireTakeoverSettings {
    #[serde(rename = "leadTime", serialize_with = "f64_swift")]
    /// How long before a meeting starts its takeover fires, in seconds.
    pub lead_time: f64,
    #[serde(rename = "takeoverCalendarKeys")]
    /// Keys of the calendars that may produce takeovers.
    pub takeover_calendar_keys: Vec<String>,
    #[serde(rename = "hiddenCalendarKeys")]
    /// Keys of the calendars hidden from the agenda; a hidden calendar never produces a takeover.
    pub hidden_calendar_keys: Vec<String>,
    #[serde(rename = "requireConferenceLink")]
    /// True to give takeovers only to meetings that have a video-meeting link.
    pub require_conference_link: bool,
    #[serde(rename = "requireOtherAttendees")]
    /// True to give takeovers only to meetings that have other attendees.
    pub require_other_attendees: bool,
    #[serde(rename = "skipAllDayEvents")]
    /// True to give no takeovers for all-day events.
    pub skip_all_day_events: bool,
    /// Master switch for takeovers.
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// The settings the engine shows.
pub struct WireSettings {
    /// Takeover settings.
    pub takeover: WireTakeoverSettings,
    #[serde(rename = "inferenceEnabled")]
    /// True when the text model may judge ambiguous duplicates.
    pub inference_enabled: bool,
    #[serde(rename = "systemCalendarEnabled")]
    /// True when the system calendar is a source.
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
    /// Replaces the lead time, in seconds.
    pub lead_time: Option<f64>,
    #[serde(
        default,
        rename = "takeoverCalendarKeys",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces the list of calendar keys that may produce takeovers.
    pub takeover_calendar_keys: Option<Vec<String>>,
    #[serde(
        default,
        rename = "hiddenCalendarKeys",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces the list of hidden calendar keys; hidden calendars also lose their takeover opt-in.
    pub hidden_calendar_keys: Option<Vec<String>>,
    #[serde(
        default,
        rename = "requireConferenceLink",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces the conference link requirement.
    pub require_conference_link: Option<bool>,
    #[serde(
        default,
        rename = "requireOtherAttendees",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces the other-attendees requirement.
    pub require_other_attendees: Option<bool>,
    #[serde(
        default,
        rename = "skipAllDayEvents",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces the all-day skip.
    pub skip_all_day_events: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Replaces the takeover master switch.
    pub enabled: Option<bool>,
    #[serde(
        default,
        rename = "inferenceEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces whether the text model may judge duplicates.
    pub inference_enabled: Option<bool>,
    #[serde(
        default,
        rename = "systemCalendarEnabled",
        skip_serializing_if = "Option::is_none"
    )]
    /// Replaces whether the system calendar is a source.
    pub system_calendar_enabled: Option<bool>,
}

// MARK: - State and events

/// The whole `EngineState`, field for field. `FIELD_NAMES` is the coding-key list itself, so a test can compare it with
/// the engine type's fields and a later engine field cannot be forgotten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireState {
    /// Starts at 0 and increases with every published state.
    pub revision: u64,
    /// The day agenda.
    pub agenda: WireAgenda,
    /// Deduplicated events from all sources.
    pub events: Vec<WireEvent>,
    /// Every calendar the sources offer.
    pub calendars: Vec<WireCalendar>,
    /// Health of each source, by source id.
    pub statuses: BTreeMap<String, WireSourceStatus>,
    /// Quiet hints per source, by source id; a source with none has no entry.
    pub notices: BTreeMap<String, Vec<WireNotice>>,
    #[serde(rename = "sourceNames")]
    /// Display name of each source, by source id.
    pub source_names: BTreeMap<String, String>,
    /// Pending duplicate pairs: an event id to the ids of the events it may duplicate; all resolve in `events`.
    pub candidates: BTreeMap<String, Vec<String>>,
    /// The stored accounts.
    pub accounts: Vec<WireAccount>,
    #[serde(rename = "accountStatus")]
    /// Sign-in progress and errors.
    pub account_status: WireAccountStatus,
    /// The current settings.
    pub settings: WireSettings,
    #[serde(rename = "inferenceStatus")]
    /// Whether the text model is in use.
    pub inference_status: WireInferenceStatus,
    /// Summary of the takeover ledger.
    pub ledger: WireLedger,
    #[serde(
        default,
        rename = "nextTakeover",
        skip_serializing_if = "Option::is_none"
    )]
    /// The armed takeover; absent when none is armed (also while one is on screen).
    pub next_takeover: Option<WireNextTakeover>,
    #[serde(rename = "takeoverVisible")]
    /// True while a takeover is shown.
    pub takeover_visible: bool,
    #[serde(rename = "availableAccountKinds")]
    /// The account kinds the person can add, in display order.
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
/// Payload of `event.takeoverDue`: what the host shows in the takeover.
pub struct WireTakeoverRequest {
    /// The meeting.
    pub event: WireEvent,
    #[serde(rename = "hasStarted")]
    /// True when the meeting has already begun.
    pub has_started: bool,
    #[serde(default, rename = "joinURL", skip_serializing_if = "Option::is_none")]
    /// The link to open on Join; absent when the event has no meeting link.
    pub join_url: Option<String>,
    /// Seconds.
    #[serde(rename = "snoozeOptions", serialize_with = "f64_swift_vec")]
    pub snooze_options: Vec<f64>,
    #[serde(
        default,
        rename = "calendarTitle",
        skip_serializing_if = "Option::is_none"
    )]
    /// Title of the event's calendar, if known.
    pub calendar_title: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// One event in a widget snapshot.
pub struct WireWidgetEvent {
    /// The engine's id for the event.
    pub id: String,
    /// Event title.
    pub title: String,
    /// Start instant, `WireDates` format; for an all-day event the local midnight of its first day.
    pub start: String,
    /// End instant, `WireDates` format; for an all-day event the local midnight of its end date.
    pub end: String,
    #[serde(rename = "isAllDay")]
    /// True for an all-day event.
    pub is_all_day: bool,
    #[serde(default, rename = "colorHex", skip_serializing_if = "Option::is_none")]
    /// Calendar color as `#RRGGBB`, if known.
    pub color_hex: Option<String>,
    #[serde(default, rename = "joinURL", skip_serializing_if = "Option::is_none")]
    /// The link to open to join the meeting, if the event has one.
    pub join_url: Option<String>,
}

/// A view model for widgets: all-day events carry the local midnights of their dates, as instants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireWidgetSnapshot {
    #[serde(rename = "generatedAt")]
    /// When the snapshot was made, `WireDates` format.
    pub generated_at: String,
    /// The events the widget shows.
    pub events: Vec<WireWidgetEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `event.diagnostic`: a log line from the engine or a rejected command.
pub struct WireDiagnostic {
    /// `info`, `warning` or `error`.
    pub level: String,
    /// Short area name such as `command`, `ledger` or `takeover`.
    pub category: String,
    /// Text of the line; contains no calendar content.
    pub message: String,
}
