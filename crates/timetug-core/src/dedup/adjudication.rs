use std::collections::BTreeMap;

use async_trait::async_trait;
use calendar_core::time::seconds_between;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::dedup::lesson_book::{Lesson, LessonBook, LessonDecision};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::text::{grapheme_count, graphemes, prefix_graphemes};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: String,
    /// Product name, e.g. "Apple Intelligence"; the front end words the badge around it.
    pub display_name: String,
    pub is_on_device: bool,
}

impl EngineInfo {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        is_on_device: bool,
    ) -> EngineInfo {
        EngineInfo {
            id: id.into(),
            display_name: display_name.into(),
            is_on_device,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdjudicatorAvailability {
    Available(EngineInfo),
    Unavailable { reason: String },
}

/// The fields of one event a model may see. No emails; notes are truncated.
#[derive(Clone, Debug, PartialEq)]
pub struct AdjudicationEvent {
    pub title: String,
    pub start: Timestamp,
    pub end: Timestamp,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub attendee_names: Vec<String>,
    pub calendar_title: Option<String>,
}

impl AdjudicationEvent {
    pub const MAX_TITLE_LENGTH: usize = 256;
    pub const MAX_LOCATION_LENGTH: usize = 512;
    pub const MAX_CALENDAR_LENGTH: usize = 128;
    pub const MAX_ATTENDEE_NAMES: usize = 10;
    pub const MAX_ATTENDEE_NAME_LENGTH: usize = 80;
    pub const MAX_NOTES_LENGTH: usize = 500;

    pub fn new(event: &TimeTugCalendarEvent, calendar: Option<&CalendarInfo>) -> AdjudicationEvent {
        // Some calendar sources put the address in the name field, so drop anything email-shaped.
        let names: Vec<String> = event
            .attendees()
            .into_iter()
            .filter_map(|a| a.name.as_deref())
            .filter(|name| !name.is_empty() && !name.contains('@'))
            .take(Self::MAX_ATTENDEE_NAMES)
            .map(|name| prefix_graphemes(name, Self::MAX_ATTENDEE_NAME_LENGTH))
            .collect();
        AdjudicationEvent {
            title: prefix_graphemes(event.title(), Self::MAX_TITLE_LENGTH),
            start: event.start(),
            end: event.end(),
            location: event
                .location()
                .map(|v| prefix_graphemes(v, Self::MAX_LOCATION_LENGTH)),
            notes: event
                .notes()
                .map(|v| prefix_graphemes(v, Self::MAX_NOTES_LENGTH)),
            attendee_names: names,
            calendar_title: calendar
                .map(|info| prefix_graphemes(&info.title, Self::MAX_CALENDAR_LENGTH)),
        }
    }

    pub fn bounded(&self) -> AdjudicationEvent {
        let mut value = self.clone();
        value.title = prefix_graphemes(&self.title, Self::MAX_TITLE_LENGTH);
        value.location = self
            .location
            .as_deref()
            .map(|v| prefix_graphemes(v, Self::MAX_LOCATION_LENGTH));
        value.notes = self
            .notes
            .as_deref()
            .map(|v| prefix_graphemes(v, Self::MAX_NOTES_LENGTH));
        value.calendar_title = self
            .calendar_title
            .as_deref()
            .map(|v| prefix_graphemes(v, Self::MAX_CALENDAR_LENGTH));
        value.attendee_names = self
            .attendee_names
            .iter()
            .filter(|name| !name.is_empty() && !name.contains('@'))
            .take(Self::MAX_ATTENDEE_NAMES)
            .map(|name| prefix_graphemes(name, Self::MAX_ATTENDEE_NAME_LENGTH))
            .collect();
        value
    }

    pub(crate) fn prompt_bounded(&self) -> AdjudicationEvent {
        let mut value = self.bounded();
        value.title = PromptText::bounded(&value.title, 350);
        value.location = value.location.map(|v| PromptText::bounded(&v, 400));
        value.notes = value.notes.map(|v| PromptText::bounded(&v, 400));
        value.calendar_title = value.calendar_title.map(|v| PromptText::bounded(&v, 160));
        value.attendee_names = value
            .attendee_names
            .iter()
            .map(|name| PromptText::bounded(name, 50))
            .collect();
        value
    }

    /// What `JSONEncoder` with `.sortedKeys` and `.millisecondsSince1970` writes for the event: keys in order, absent
    /// optionals omitted, dates as milliseconds since 1970 (not the reference-date numbers of the state files).
    pub(crate) fn canonical_json(&self) -> String {
        let mut out = String::from("{\"attendeeNames\":[");
        out.push_str(
            &self
                .attendee_names
                .iter()
                .map(|name| json_string(name))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push(']');
        if let Some(title) = &self.calendar_title {
            out.push_str(&format!(",\"calendarTitle\":{}", json_string(title)));
        }
        out.push_str(&format!(",\"end\":{}", json_millis(self.end)));
        if let Some(location) = &self.location {
            out.push_str(&format!(",\"location\":{}", json_string(location)));
        }
        if let Some(notes) = &self.notes {
            out.push_str(&format!(",\"notes\":{}", json_string(notes)));
        }
        out.push_str(&format!(",\"start\":{}", json_millis(self.start)));
        out.push_str(&format!(",\"title\":{}}}", json_string(&self.title)));
        out
    }
}

/// A JSON string as Swift's `JSONEncoder` writes it: `"`, `\` and `/` escaped, the C0 controls as `\b \t \n \f \r` or
/// `\u00xx`, everything else (including non-ASCII) as it is.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `.millisecondsSince1970`: `timeIntervalSince1970 * 1000` as a JSON number, an integer when it is one.
fn json_millis(t: Timestamp) -> String {
    let millis = t.as_nanosecond() as f64 / 1e6;
    if millis.fract() == 0.0 && millis.abs() < 9.0e15 {
        format!("{}", millis as i64)
    } else {
        format!("{millis}")
    }
}

/// Shared escaping and length policy for canonical model input and its rendered prompt.
pub struct PromptText;

impl PromptText {
    pub fn quoted(value: &str) -> String {
        json_string(value)
            .replace('<', "\\u003C")
            .replace('>', "\\u003E")
    }

    pub fn bounded(value: &str, quoted_limit: usize) -> String {
        if grapheme_count(&Self::quoted(value)) <= quoted_limit {
            return value.to_string();
        }
        let mut prefix = String::new();
        for character in graphemes(value) {
            let next = format!("{prefix}{character}");
            if grapheme_count(&Self::quoted(&next)) > quoted_limit - 3 {
                break;
            }
            prefix = next;
        }
        prefix + "…"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdjudicationRequest {
    /// Fingerprint of both events' relevant content (not their calendars); the cache key.
    pub id: String,
    /// The more detailed event (ties: the earlier one); the other is `second`.
    pub first: AdjudicationEvent,
    pub second: AdjudicationEvent,
    pub lessons: Vec<Lesson>,
    /// Rule-computed facts about the pair, free of display strings. Offsets are `second` minus `first`.
    pub start_offset_minutes: i64,
    pub end_offset_minutes: i64,
    pub overlap_minutes: i64,
    /// `DuplicateRules::detail_summary` of each event, e.g. "bare" or "location+notes".
    pub first_details: String,
    pub second_details: String,
    /// False for every ambiguous pair by construction; carried so the prompt can say so.
    pub has_conflicting_details: bool,
}

impl AdjudicationRequest {
    /// Swift's `init(id:first:second:lessons:)` with the offsets, overlap and flag at their defaults (0, 0, 0, "bare",
    /// "bare", false); assign the public fields for the rest.
    pub fn new(
        id: impl Into<String>,
        first: AdjudicationEvent,
        second: AdjudicationEvent,
        lessons: Vec<Lesson>,
    ) -> AdjudicationRequest {
        AdjudicationRequest {
            id: id.into(),
            first,
            second,
            lessons,
            start_offset_minutes: 0,
            end_offset_minutes: 0,
            overlap_minutes: 0,
            first_details: "bare".to_string(),
            second_details: "bare".to_string(),
            has_conflicting_details: false,
        }
    }

    pub fn input(&self) -> JudgmentInput {
        JudgmentInput::new(self)
    }
}

/// The exact bounded data supplied to the model and used to identify a verdict. The policy version covers
/// instruction and formatting changes; no volatile lesson timestamp enters the cache key.
#[derive(Clone, Debug, PartialEq)]
pub struct JudgmentInput {
    pub first: AdjudicationEvent,
    pub second: AdjudicationEvent,
    pub lessons: Vec<Correction>,
    pub start_offset_minutes: i64,
    pub end_offset_minutes: i64,
    pub overlap_minutes: i64,
    pub first_details: String,
    pub second_details: String,
    pub has_conflicting_details: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    pub title_a: String,
    pub title_b: String,
    pub signals_a: String,
    pub signals_b: String,
    pub decision: LessonDecision,
}

impl Correction {
    fn canonical_json(&self) -> String {
        format!(
            "{{\"decision\":{},\"signalsA\":{},\"signalsB\":{},\"titleA\":{},\"titleB\":{}}}",
            json_string(match self.decision {
                LessonDecision::Same => "same",
                LessonDecision::Different => "different",
            }),
            json_string(&self.signals_a),
            json_string(&self.signals_b),
            json_string(&self.title_a),
            json_string(&self.title_b)
        )
    }
}

impl JudgmentInput {
    pub const POLICY_VERSION: i64 = 2;
    pub const MAX_LESSON_LENGTH: usize = 256;

    pub fn new(request: &AdjudicationRequest) -> JudgmentInput {
        let bounded = |text: &str, limit: usize| {
            PromptText::bounded(&prefix_graphemes(text, Self::MAX_LESSON_LENGTH), limit)
        };
        JudgmentInput {
            first: request.first.prompt_bounded(),
            second: request.second.prompt_bounded(),
            lessons: request
                .lessons
                .iter()
                .take(LessonBook::PROMPT_LIMIT)
                .map(|lesson| Correction {
                    title_a: bounded(&lesson.title_a, 50),
                    title_b: bounded(&lesson.title_b, 50),
                    signals_a: bounded(&lesson.signals_a, 30),
                    signals_b: bounded(&lesson.signals_b, 30),
                    decision: lesson.decision,
                })
                .collect(),
            start_offset_minutes: request.start_offset_minutes,
            end_offset_minutes: request.end_offset_minutes,
            overlap_minutes: request.overlap_minutes,
            first_details: request.first_details.clone(),
            second_details: request.second_details.clone(),
            has_conflicting_details: request.has_conflicting_details,
        }
    }

    /// `cacheKey(engineID:)` with the current policy version.
    pub fn cache_key(&self, engine_id: &str) -> String {
        self.cache_key_with_policy(engine_id, Self::POLICY_VERSION)
    }

    pub fn cache_key_with_policy(&self, engine_id: &str, policy_version: i64) -> String {
        let side = |event: &AdjudicationEvent, details: &str| {
            format!(
                "{{\"details\":{},\"event\":{}}}",
                json_string(details),
                event.canonical_json()
            )
        };
        let mut sides = [
            side(&self.first, &self.first_details),
            side(&self.second, &self.second_details),
        ];
        sides.sort();
        let mut corrections: Vec<String> = self
            .lessons
            .iter()
            .map(Correction::canonical_json)
            .collect();
        corrections.sort();
        let mut parts = vec![policy_version.to_string(), engine_id.to_string()];
        parts.extend(sides);
        parts.extend(corrections);
        parts.extend([
            self.start_offset_minutes.abs().to_string(),
            self.end_offset_minutes.abs().to_string(),
            self.overlap_minutes.to_string(),
            self.has_conflicting_details.to_string(),
        ]);
        let joined: String = parts
            .iter()
            .map(|part| format!("{}:{}", part.len(), part))
            .collect();
        format!("j{policy_version}-{}", Fingerprint::fnv1a(&joined))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictAnswer {
    Same,
    Different,
    Unsure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdjudicationVerdict {
    pub request_id: String,
    pub answer: VerdictAnswer,
}

impl AdjudicationVerdict {
    pub fn new(request_id: impl Into<String>, answer: VerdictAnswer) -> AdjudicationVerdict {
        AdjudicationVerdict {
            request_id: request_id.into(),
            answer,
        }
    }
}

/// A platform's on-device model. Implementations live outside Core.
#[async_trait]
pub trait DuplicateAdjudicator: Send + Sync {
    fn availability(&self) -> AdjudicatorAvailability;
    /// Verdicts for the requests it could judge; omit a request on failure so it is retried later.
    async fn judge(&self, requests: &[AdjudicationRequest]) -> Vec<AdjudicationVerdict>;
}

/// One cached verdict (Swift's `VerdictCache.Entry`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerdictEntry {
    pub answer: VerdictAnswer,
    pub engine: EngineInfo,
    #[serde(with = "calendar_core::json::swift_reference_date")]
    pub decided_at: Timestamp,
    #[serde(with = "calendar_core::json::swift_reference_date")]
    pub end: Timestamp,
}

/// Model verdicts, kept so each pair is judged once. Entries expire by age (`RETENTION`) and the
/// cap, never by event end: the fetch window includes events that already ended today, and dropping
/// their verdicts would make the pair pending (and judged) again on every pass.
///
/// Persisted (family A): dates are reference-date numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictCache {
    entries: BTreeMap<String, VerdictEntry>,
}

impl VerdictCache {
    pub const RETENTION: f64 = 7.0 * 24.0 * 60.0 * 60.0;
    pub const MAX_ENTRIES: usize = 1000;

    pub fn entries(&self) -> &BTreeMap<String, VerdictEntry> {
        &self.entries
    }

    /// `entry(for:)`.
    pub fn entry_for(&self, request_id: &str) -> Option<&VerdictEntry> {
        self.entries.get(request_id)
    }

    pub fn store(
        &mut self,
        verdict: &AdjudicationVerdict,
        engine: &EngineInfo,
        end: Timestamp,
        now: Timestamp,
    ) {
        self.entries.insert(
            verdict.request_id.clone(),
            VerdictEntry {
                answer: verdict.answer,
                engine: engine.clone(),
                decided_at: now,
                end,
            },
        );
    }

    pub fn discard_incompatible(&mut self) {
        let prefix = format!("j{}-", JudgmentInput::POLICY_VERSION);
        self.entries.retain(|key, _| key.starts_with(&prefix));
    }

    /// True if anything was dropped.
    pub fn prune(&mut self, now: Timestamp) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| seconds_between(entry.decided_at, now) <= Self::RETENTION);
        if self.entries.len() > Self::MAX_ENTRIES {
            let mut oldest_first: Vec<(Timestamp, String)> = self
                .entries
                .iter()
                .map(|(key, entry)| (entry.decided_at, key.clone()))
                .collect();
            oldest_first.sort();
            let excess = self.entries.len() - Self::MAX_ENTRIES;
            for (_, key) in oldest_first.into_iter().take(excess) {
                self.entries.remove(&key);
            }
        }
        self.entries.len() != before
    }
}

pub(crate) struct Fingerprint;

impl Fingerprint {
    /// 64-bit FNV-1a as hex. Deterministic across launches and platforms (unlike `Hasher`).
    pub(crate) fn fnv1a(text: &str) -> String {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{hash:x}")
    }
}
