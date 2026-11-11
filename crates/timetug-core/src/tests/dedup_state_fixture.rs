//! `Tests/TimeTugEngineTests/Fixtures/dedup-state.json`: the persisted lesson book and verdict cache (family A,
//! reference-date numbers) and the cache-key encoding, which is the other date encoding (milliseconds since 1970).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::dedup_support::ev;
use super::support::date;
use crate::dedup::adjudication::{
    AdjudicationEvent, AdjudicationRequest, EngineInfo, PromptText, VerdictAnswer, VerdictCache,
};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};

const FIXTURE: &str =
    include_str!("../../../../Tests/TimeTugEngineTests/Fixtures/dedup-state.json");

#[derive(Serialize, Deserialize)]
struct DedupState {
    lessons: LessonBook,
    verdicts: VerdictCache,
}

#[test]
fn the_committed_dedup_state_loads_with_reference_date_seconds() {
    let state: DedupState = serde_json::from_str(FIXTURE).unwrap();
    // 821692800 seconds after 2001-01-01 is 2027-01-15T08:00:00Z, that is 1800000000 Unix seconds.
    let lesson = &state.lessons.lessons()[0];
    assert_eq!(state.lessons.lessons().len(), 1);
    assert_eq!(lesson.title_a, "sample a");
    assert_eq!(lesson.title_b, "sample b");
    assert_eq!(lesson.calendar_key_a, "s/x");
    assert_eq!(lesson.calendar_key_b, "s/y");
    assert_eq!(lesson.decision, LessonDecision::Same);
    assert_eq!(lesson.last_used.as_second(), 1_800_000_000);
    assert_eq!(lesson.content_key_a, None);
    let entry = state.verdicts.entry_for("r").unwrap();
    assert_eq!(entry.answer, VerdictAnswer::Same);
    assert_eq!(entry.engine, EngineInfo::new("e", "E", true));
    assert_eq!(entry.decided_at.as_second(), 1_800_000_000);
    assert_eq!(entry.end.as_second(), 1_800_003_600);
}

#[test]
fn the_dedup_state_saves_to_the_same_json_and_reloads() {
    let state: DedupState = serde_json::from_str(FIXTURE).unwrap();
    let saved = serde_json::to_string_pretty(&state).unwrap();
    let expected: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&saved).unwrap(), expected);
    let reloaded: DedupState = serde_json::from_str(&saved).unwrap();
    assert_eq!(reloaded.lessons, state.lessons);
    assert_eq!(reloaded.verdicts, state.verdicts);
}

#[test]
fn the_cache_key_encodes_dates_as_milliseconds_since_1970_unlike_the_state_file() {
    // 2026-09-18T10:00:00Z (the default test event) is 1789725600 Unix seconds, and it lasts 30 minutes.
    let event = AdjudicationEvent::new(&ev("1").title("Doctor").build(), None);
    let json = event.canonical_json();
    assert_eq!(
        json,
        r#"{"attendeeNames":[],"end":1789727400000,"start":1789725600000,"title":"Doctor"}"#
    );
    assert_eq!(date("2026-09-18T10:00:00Z").as_second(), 1_789_725_600);
    // And the persisted form of the same instant is reference-date seconds.
    let mut cache = VerdictCache::default();
    cache.store(
        &crate::dedup::adjudication::AdjudicationVerdict::new("x", VerdictAnswer::Same),
        &EngineInfo::new("e", "E", true),
        date("2026-09-18T10:00:00Z"),
        date("2026-09-18T10:00:00Z"),
    );
    let saved: Value = serde_json::to_value(&cache).unwrap();
    assert_eq!(saved["entries"]["x"]["decidedAt"], 811_418_400);
}

#[test]
fn prompt_text_quotes_like_swifts_json_encoder_and_escapes_angle_brackets() {
    assert_eq!(PromptText::quoted("plain"), "\"plain\"");
    assert_eq!(
        PromptText::quoted("a/b \"q\" \\ \n\t<x>"),
        r#""a\/b \"q\" \\ \n\t\u003Cx\u003E""#
    );
    assert_eq!(PromptText::quoted("\u{1}é"), "\"\\u0001é\"");
    assert_eq!(PromptText::bounded("short", 100), "short");
    let bounded = PromptText::bounded(&"x".repeat(100), 20);
    // The quoted text may be 20 - 3 = 17 characters: 15 letters between the two quotes.
    assert_eq!(bounded, format!("{}…", "x".repeat(15)));
}

#[test]
fn the_request_fingerprint_is_the_policy_version_and_sixteen_hex_digits() {
    let a = AdjudicationEvent::new(&ev("1").title("Doctor").build(), None);
    let b = AdjudicationEvent::new(&ev("2").title("Clinic").build(), None);
    let key = AdjudicationRequest::new("", a, b, vec![])
        .input()
        .cache_key("apple");
    assert!(key.starts_with("j2-"), "{key}");
    assert!(key[3..].len() <= 16 && key[3..].chars().all(|c| c.is_ascii_hexdigit()));
}
