use std::collections::BTreeMap;

use calendar_core::all_day::AllDay;
use calendar_core::civil::{self, Unit};
use calendar_core::time::{Zone, add_seconds, seconds_between};
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;

/// Dates as `JSONEncoder.dateEncodingStrategy = .iso8601` writes them (`WidgetSnapshotStore` sets it at the write
/// site, so this file is not family A): `2026-09-18T12:00:00Z`, UTC, whole seconds (a fraction is dropped by flooring,
/// as `ISO8601DateFormatter` does). Reading accepts any offset and an optional fraction, as the Swift decoder does.
mod iso8601 {
    use jiff::Timestamp;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(t: &Timestamp, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seconds = t.as_second();
        if t.subsec_nanosecond() < 0 {
            seconds -= 1;
        }
        let floored = Timestamp::from_second(seconds).map_err(serde::ser::Error::custom)?;
        serializer.collect_str(&floored)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Timestamp, D::Error> {
        let text = String::deserialize(deserializer)?;
        // ISO 8601 as `ISO8601DateFormatter` reads it: the date and time are joined by `T` (jiff also accepts a space).
        if text.as_bytes().get(10) != Some(&b'T') {
            return Err(D::Error::custom("not an ISO 8601 date-time"));
        }
        text.parse::<Timestamp>().map_err(D::Error::custom)
    }
}

/// One event as a widget needs it. Times are the ones shown to the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetEvent {
    /// The event's id.
    pub id: String,
    /// The event's title.
    pub title: String,
    /// Start as shown to the user (for an all-day event, local midnight of its first date).
    #[serde(with = "iso8601")]
    pub start: Timestamp,
    /// End as shown to the user (for an all-day event, local midnight of the exclusive end date).
    #[serde(with = "iso8601")]
    pub end: Timestamp,
    /// True for an all-day event.
    pub is_all_day: bool,
    /// The event's calendar color as `#RRGGBB`; `None` when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_hex: Option<String>,
    /// The URL to join the meeting, when the event has one.
    #[serde(default, rename = "joinURL", skip_serializing_if = "Option::is_none")]
    pub join_url: Option<UrlString>,
}

impl WidgetEvent {
    /// Swift's `init`; its defaults are `isAllDay: false`, `colorHex: nil`, `joinURL: nil`.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        start: Timestamp,
        end: Timestamp,
        is_all_day: bool,
        color_hex: Option<String>,
        join_url: Option<UrlString>,
    ) -> WidgetEvent {
        WidgetEvent {
            id: id.into(),
            title: title.into(),
            start,
            end,
            is_all_day,
            color_hex,
            join_url,
        }
    }
}

/// The agenda a front end hands to widgets: today plus the next days, already merged, filtered and sorted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSnapshot {
    /// When the snapshot was made; see [`WidgetSnapshot::is_stale`].
    #[serde(with = "iso8601")]
    pub generated_at: Timestamp,
    /// The events of the horizon, sorted by start, then title, then id.
    pub events: Vec<WidgetEvent>,
}

impl WidgetSnapshot {
    /// Days of events included, counting today.
    pub const HORIZON_DAYS: i64 = 3;
    /// A snapshot older than this is treated as missing by widgets.
    pub const MAX_AGE: f64 = 12.0 * 3600.0;

    /// Creates a snapshot made at `generated_at` holding `events` as given.
    pub fn new(generated_at: Timestamp, events: Vec<WidgetEvent>) -> WidgetSnapshot {
        WidgetSnapshot {
            generated_at,
            events,
        }
    }

    /// True when the snapshot is older than [`WidgetSnapshot::MAX_AGE`] at `now`.
    pub fn is_stale(&self, now: Timestamp) -> bool {
        seconds_between(self.generated_at, now) > WidgetSnapshot::MAX_AGE
    }

    /// Builds the snapshot at `now` in `zone` from merged `events`. Events on hidden calendars and, when
    /// `skip_all_day_events` is set, all-day events are left out; colors come from `calendars`. Events that
    /// overlap the next [`WidgetSnapshot::HORIZON_DAYS`] days are kept.
    pub fn make(
        events: &[TimeTugCalendarEvent],
        calendars: &[CalendarInfo],
        settings: &TakeoverSettings,
        now: Timestamp,
        zone: &Zone,
    ) -> WidgetSnapshot {
        let day_start = civil::start_of_day(now, zone);
        // `calendar.date(byAdding: .day, ...)!`: Swift traps outside the range of a `Date`; 24 hours per day instead.
        let horizon_end = civil::add(Unit::Day, WidgetSnapshot::HORIZON_DAYS, day_start, zone)
            .unwrap_or_else(|| {
                add_seconds(day_start, 86_400.0 * WidgetSnapshot::HORIZON_DAYS as f64)
            });
        // `uniquingKeysWith: { first, _ in first }`
        let mut colors: BTreeMap<String, String> = BTreeMap::new();
        for info in calendars {
            if let Some(hex) = info.color_hex() {
                colors.entry(info.key()).or_insert_with(|| hex.to_string());
            }
        }
        let today = AllDay::date_of(day_start, zone);
        let horizon = AllDay::date_of(horizon_end, zone);
        let mut included: Vec<WidgetEvent> = events
            .iter()
            .filter(|e| {
                !e.all_calendar_keys()
                    .is_subset(&settings.hidden_calendar_keys)
            })
            .filter(|e| !(settings.skip_all_day_events && e.is_all_day()))
            .filter(|event| {
                if let Some((first, end_exclusive)) = event.all_day_dates() {
                    return end_exclusive > today && first < horizon;
                }
                event.end() > day_start && event.shown_start() < horizon_end
            })
            .map(|event| {
                let (mut start, mut end) = (event.shown_start(), event.end());
                if let Some((first, end_exclusive)) = event.all_day_dates()
                    && let (Some(local_start), Some(local_end)) = (
                        AllDay::start_of_day(&first, zone),
                        AllDay::start_of_day(&end_exclusive, zone),
                    )
                {
                    // the snapshot is a view model: local midnights for the same dates
                    (start, end) = (local_start, local_end);
                }
                WidgetEvent::new(
                    event.id(),
                    event.title(),
                    start,
                    end,
                    event.is_all_day(),
                    colors.get(&event.calendar_key()).cloned(),
                    event.conference_url().cloned(),
                )
            })
            .collect();
        // Stable, like Swift's `sorted`; titles and ids compare by unicode scalar value.
        included.sort_by(|a, b| (a.start, &a.title, &a.id).cmp(&(b.start, &b.title, &b.id)));
        WidgetSnapshot::new(now, included)
    }
}
