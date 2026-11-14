use std::collections::BTreeMap;
use std::fmt;

use calendar_core::json::swift_reference_date;
use calendar_core::time::add_seconds;
use jiff::Timestamp;
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::event::TimeTugCalendarEvent;

/// Remembers which events already took over (or are snoozed) so a refresh, or a relaunch, never
/// repeats one. Every mutation is recorded under `TimeTugCalendarEvent::id` (includes the start time,
/// so a rescheduled event is new) and every key in `all_content_keys` (survives a changed source id,
/// so a merge or split never re-fires a meeting).
/// Serializable so the app can persist it; `prune` keeps the persisted form bounded.
///
/// Persisted form (family A of the conventions, the file `takeover-ledger.json`): `{"records": {<key>: {"entry":
/// {"fired":{}} or {"snoozed":{"until":<n>}}, "end": <n>, "recordedAt": <n>}}}`, every date a number of seconds since
/// 2001-01-01. The serde code below is written by hand, case by case, after Swift's synthesized coding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TakeoverLedger {
    records: BTreeMap<String, Record>,
}

/// One remembered state of an event (Swift `TakeoverLedger.Entry`, internal there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// The takeover has been shown (or acknowledged) and must not fire again.
    Fired,
    /// The user snoozed the event; it fires again at `until`.
    Snoozed {
        /// When the snoozed takeover fires again.
        until: Timestamp,
    },
}

/// Swift `TakeoverLedger.Record`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Record {
    entry: Entry,
    end: Timestamp,
    recorded_at: Timestamp,
}

impl TakeoverLedger {
    /// Entries older than this are dropped even if the event has not "ended" (bogus end dates).
    pub const RETENTION: f64 = 7.0 * 24.0 * 60.0 * 60.0;
    /// Last-resort cap on stored keys: `2 * MAX_ENTRIES` (4000). An unmerged event stores 2 keys (its id
    /// and its content key), so that is about 2000 events; a merged event stores up to 5 (its id plus
    /// up to 4 content keys), so about 800 merged events in the worst case. Oldest go first.
    pub const MAX_ENTRIES: usize = 2000;

    /// Swift `TakeoverLedger()`.
    pub fn new() -> TakeoverLedger {
        TakeoverLedger::default()
    }

    /// Records that the event's takeover has fired, so it never fires again (including after a merge or split).
    pub fn mark_fired(&mut self, event: &TimeTugCalendarEvent, now: Timestamp) {
        self.record(Entry::Fired, event, now);
    }

    /// Re-arms the event `duration` from now, never past the meeting's end.
    pub fn snooze(&mut self, event: &TimeTugCalendarEvent, duration: f64, now: Timestamp) {
        let until = std::cmp::min(add_seconds(now, duration), event.end());
        self.record(Entry::Snoozed { until }, event, now);
    }

    /// Marks every un-ledgered event that started before `now - grace` and has not ended as fired.
    /// Used once per launch so a meeting already underway never takes over. Returns the count.
    pub fn acknowledge_in_progress(
        &mut self,
        events: &[TimeTugCalendarEvent],
        now: Timestamp,
        grace: f64,
    ) -> usize {
        let mut count = 0;
        for event in events {
            if self.entry_for(event).is_none()
                && event.start() < add_seconds(now, -grace)
                && event.end() > now
            {
                self.mark_fired(event, now);
                count += 1;
            }
        }
        count
    }

    /// Drops entries whose event has ended or that were recorded more than `RETENTION` ago (not
    /// wholesale, so midnight rollover is safe), then enforces `MAX_ENTRIES`. True if anything changed.
    pub fn prune(&mut self, now: Timestamp) -> bool {
        let before = self.records.len();
        let cutoff = add_seconds(now, -Self::RETENTION);
        self.records
            .retain(|_, record| record.end > now && record.recorded_at > cutoff);

        let limit = Self::MAX_ENTRIES * 2;
        if self.records.len() > limit {
            let mut oldest_first: Vec<(Timestamp, String)> = self
                .records
                .iter()
                .map(|(key, record)| (record.recorded_at, key.clone()))
                .collect();
            oldest_first.sort();
            let excess = self.records.len() - limit;
            for (_, key) in oldest_first.into_iter().take(excess) {
                self.records.remove(&key);
            }
        }
        self.records.len() != before
    }

    /// True only for the `Fired` state; a snoozed event is pending.
    pub fn has_fired(&self, event: &TimeTugCalendarEvent) -> bool {
        self.entry_for(event) == Some(Entry::Fired)
    }

    /// True while the event is snoozed (pending, not fired).
    pub fn is_snoozed(&self, event: &TimeTugCalendarEvent) -> bool {
        matches!(self.entry_for(event), Some(Entry::Snoozed { .. }))
    }

    /// Number of stored keys (per remembered event: its id key plus one content key per merged member, at most 5). For diagnostics.
    pub fn key_count(&self) -> usize {
        self.records.len()
    }

    /// The entry found through the event's id or any of its content keys (tried in that order, the content keys
    /// sorted); a fired entry wins over a snoozed one.
    pub(crate) fn entry_for(&self, event: &TimeTugCalendarEvent) -> Option<Entry> {
        let mut keys = vec![event.id()];
        // `all_content_keys` is a `BTreeSet`: sorted, as Swift's `.sorted()`.
        keys.extend(event.all_content_keys());
        let found: Vec<Entry> = keys
            .iter()
            .filter_map(|key| self.records.get(key).map(|record| record.entry))
            .collect();
        found
            .iter()
            .find(|entry| **entry == Entry::Fired)
            .or(found.first())
            .copied()
    }

    fn record(&mut self, entry: Entry, event: &TimeTugCalendarEvent, now: Timestamp) {
        let record = Record {
            entry,
            end: event.end(),
            recorded_at: now,
        };
        self.records.insert(event.id(), record);
        for key in event.all_content_keys() {
            self.records.insert(key, record);
        }
    }
}

// MARK: Serialization

/// A date as the number Swift's default `Date` coding writes.
struct ReferenceDate(Timestamp);

impl Serialize for ReferenceDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        swift_reference_date::serialize(&self.0, serializer)
    }
}

/// `{}`: the nested container of a case without values.
struct EmptyObject;

impl Serialize for EmptyObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_map(Some(0))?.end()
    }
}

/// `{"until": <date>}`.
struct SnoozedBody(Timestamp);

impl Serialize for SnoozedBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Snoozed", 1)?;
        state.serialize_field("until", &ReferenceDate(self.0))?;
        state.end()
    }
}

impl Serialize for Entry {
    /// Swift's synthesized layout: `{"fired":{}}` and `{"snoozed":{"until":<date>}}`.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Entry::Fired => map.serialize_entry("fired", &EmptyObject)?,
            Entry::Snoozed { until } => map.serialize_entry("snoozed", &SnoozedBody(*until))?,
        }
        map.end()
    }
}

impl Serialize for Record {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("Record", 3)?;
        state.serialize_field("end", &ReferenceDate(self.end))?;
        state.serialize_field("entry", &self.entry)?;
        state.serialize_field("recordedAt", &ReferenceDate(self.recorded_at))?;
        state.end()
    }
}

impl Serialize for TakeoverLedger {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("TakeoverLedger", 1)?;
        state.serialize_field("records", &self.records)?;
        state.end()
    }
}

/// One JSON value, read completely and kept just far enough to look at. A record that is malformed must be skipped
/// without breaking the parse of the records after it (Swift's `LenientRecord`), so a record is first read as
/// `Raw`, which cannot fail, and interpreted afterwards.
enum Raw {
    Null,
    Bool,
    Number(f64),
    Text,
    List,
    Object(Vec<(String, Raw)>),
}

impl Raw {
    /// The members of an object; `None` for any other kind of value (Swift: `container(keyedBy:)` throws).
    fn members(&self) -> Option<&[(String, Raw)]> {
        match self {
            Raw::Object(members) => Some(members),
            _ => None,
        }
    }

    /// The value of `key` in an object (the last one when the key repeats, as a JSON parser's dictionary keeps).
    fn member(&self, key: &str) -> Option<&Raw> {
        self.members()?
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// `decode(Date.self, forKey:)` of Swift's default strategy: a number of seconds since 2001-01-01.
    fn date(&self, key: &str) -> Option<Timestamp> {
        match self.member(key)? {
            Raw::Number(seconds) => swift_reference_date::from_number(*seconds),
            _ => None,
        }
    }

    /// The synthesized `Entry.init(from:)`: exactly one of the case keys must be present (`allKeys.count == 1`); a
    /// case's value is a nested container (an object); unknown keys are ignored.
    fn entry(&self) -> Option<Entry> {
        let fired = self.member("fired");
        let snoozed = self.member("snoozed");
        match (fired, snoozed) {
            (Some(body), None) => {
                body.members()?;
                Some(Entry::Fired)
            }
            (None, Some(body)) => {
                body.members()?;
                Some(Entry::Snoozed {
                    until: body.date("until")?,
                })
            }
            _ => None,
        }
    }

    /// The synthesized `Record.init(from:)`: all three members are required.
    fn record(&self) -> Option<Record> {
        self.members()?;
        Some(Record {
            entry: self.member("entry")?.entry()?,
            end: self.date("end")?,
            recorded_at: self.date("recordedAt")?,
        })
    }
}

impl<'de> Deserialize<'de> for Raw {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Raw, D::Error> {
        struct RawVisitor;

        impl<'de> Visitor<'de> for RawVisitor {
            type Value = Raw;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Raw, E> {
                Ok(Raw::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<Raw, E> {
                Ok(Raw::Null)
            }
            fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Raw, D::Error> {
                Raw::deserialize(deserializer)
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Raw, E> {
                Ok(Raw::Bool)
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Raw, E> {
                Ok(Raw::Number(value as f64))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Raw, E> {
                Ok(Raw::Number(value as f64))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Raw, E> {
                Ok(Raw::Number(value))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Raw, E> {
                Ok(Raw::Text)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Raw, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Raw::List)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Raw, A::Error> {
                let mut members = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    members.push((key, map.next_value::<Raw>()?));
                }
                Ok(Raw::Object(members))
            }
        }

        deserializer.deserialize_any(RawVisitor)
    }
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Entry, D::Error> {
        Raw::deserialize(deserializer)?.entry().ok_or_else(|| {
            de::Error::custom("an entry is an object with exactly one of \"fired\" and \"snoozed\"")
        })
    }
}

impl<'de> Deserialize<'de> for Record {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Record, D::Error> {
        Raw::deserialize(deserializer)?
            .record()
            .ok_or_else(|| de::Error::custom("a record needs an entry, an end and a recordedAt"))
    }
}

impl<'de> Deserialize<'de> for TakeoverLedger {
    /// The file must be an object with a `records` object (a missing `records` is an error, as in Swift). A record that
    /// cannot be decoded, or lacks `recordedAt`, is dropped rather than trusted, and the rest are kept.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TakeoverLedger, D::Error> {
        let raw = Raw::deserialize(deserializer)?;
        let records = raw
            .member("records")
            .and_then(Raw::members)
            .ok_or_else(|| de::Error::custom("a ledger is an object with a \"records\" object"))?;
        let mut ledger = TakeoverLedger::new();
        for (key, value) in records {
            match value.record() {
                Some(record) => {
                    ledger.records.insert(key.clone(), record);
                }
                // A repeated key keeps the last value, as a dictionary built from the parsed object does.
                None => {
                    ledger.records.remove(key);
                }
            }
        }
        Ok(ledger)
    }
}
