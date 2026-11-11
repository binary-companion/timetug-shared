//! Port of `TimeTugCoreTests/AdjudicationTests.swift`.

use calendar_core::time::add_seconds;

use super::dedup_support::{attendee, ev, named_attendee};
use super::support::date;
use crate::dedup::adjudication::{
    AdjudicationEvent, AdjudicationRequest, AdjudicationVerdict, EngineInfo, Fingerprint,
    VerdictAnswer, VerdictCache,
};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::model::calendar_info::CalendarInfo;
use crate::model::merge_types::MergedMember;
use crate::text::grapheme_count;

fn t0() -> jiff::Timestamp {
    date("2026-09-18T09:00:00Z")
}

fn engine() -> EngineInfo {
    EngineInfo::new("fake-ai", "Fake AI", true)
}

fn verdict(id: &str, answer: VerdictAnswer) -> AdjudicationVerdict {
    AdjudicationVerdict::new(id, answer)
}

#[test]
fn adjudication_event_truncates_notes_and_drops_emails_by_construction() {
    let event = ev("1")
        .title("Doctor")
        .notes(&"x".repeat(900))
        .attendees(vec![
            named_attendee(Some("Kristin"), Some("k@x.com")),
            named_attendee(None, Some("n@x.com")),
        ])
        .build();
    let mut info = CalendarInfo::new("fake", "cal", "Personal");
    info.account_name = Some("iCloud".to_string());
    let projected = AdjudicationEvent::new(&event, Some(&info));
    assert_eq!(
        projected.notes.as_deref().map(grapheme_count),
        Some(AdjudicationEvent::MAX_NOTES_LENGTH)
    );
    assert_eq!(projected.attendee_names, ["Kristin"]);
    assert_eq!(projected.calendar_title.as_deref(), Some("Personal"));
}

#[test]
fn adjudication_event_drops_attendee_names_that_are_email_addresses() {
    let event = ev("1")
        .title("Doctor")
        .attendees(vec![
            named_attendee(Some("kristin@example.com"), Some("kristin@example.com")),
            named_attendee(Some("Scott <scott@example.com>"), None),
            named_attendee(Some(""), Some("e@x.com")),
            named_attendee(Some("Kristin"), Some("k@x.com")),
        ])
        .build();
    assert_eq!(
        AdjudicationEvent::new(&event, None).attendee_names,
        ["Kristin"]
    );
}

#[test]
fn verdict_cache_keeps_entries_by_age_not_by_event_end() {
    let mut cache = VerdictCache::default();
    cache.store(
        &verdict("a", VerdictAnswer::Same),
        &engine(),
        add_seconds(t0(), 3600.0),
        t0(),
    );
    cache.store(
        &verdict("b", VerdictAnswer::Unsure),
        &engine(),
        add_seconds(t0(), 60.0),
        t0(),
    );
    assert_eq!(
        cache.entry_for("a").map(|e| e.answer),
        Some(VerdictAnswer::Same)
    );
    assert_eq!(
        cache.entry_for("a").map(|e| e.engine.clone()),
        Some(engine())
    );
    let dropped_fresh = cache.prune(add_seconds(t0(), 120.0)); // "b" ended but is still fresh
    assert!(!dropped_fresh);
    assert!(cache.entry_for("b").is_some());
    let dropped_at_limit = cache.prune(add_seconds(t0(), VerdictCache::RETENTION));
    assert!(!dropped_at_limit);
    let dropped_old = cache.prune(add_seconds(t0(), VerdictCache::RETENTION + 1.0));
    assert!(dropped_old);
    assert!(cache.entry_for("a").is_none());
    assert!(cache.entry_for("b").is_none());
}

#[test]
fn verdict_cache_is_capped_oldest_first() {
    let mut cache = VerdictCache::default();
    for i in 0..(VerdictCache::MAX_ENTRIES + 3) {
        cache.store(
            &verdict(&format!("r{i}"), VerdictAnswer::Same),
            &engine(),
            add_seconds(t0(), 86_400.0),
            add_seconds(t0(), i as f64),
        );
    }
    cache.prune(t0());
    assert_eq!(cache.entries().len(), VerdictCache::MAX_ENTRIES);
    assert!(cache.entry_for("r0").is_none());
}

#[test]
fn verdict_cache_codable_round_trip() {
    let mut cache = VerdictCache::default();
    cache.store(
        &verdict("a", VerdictAnswer::Different),
        &engine(),
        add_seconds(t0(), 60.0),
        t0(),
    );
    let decoded: VerdictCache =
        serde_json::from_str(&serde_json::to_string(&cache).unwrap()).unwrap();
    assert_eq!(decoded, cache);
}

#[test]
fn fingerprint_is_stable_and_sensitive() {
    assert_eq!(Fingerprint::fnv1a("abc"), Fingerprint::fnv1a("abc"));
    assert_ne!(Fingerprint::fnv1a("abc"), Fingerprint::fnv1a("abd"));
}

#[test]
fn fingerprint_matches_known_fnv1a_vectors() {
    assert_eq!(Fingerprint::fnv1a(""), "cbf29ce484222325");
    assert_eq!(Fingerprint::fnv1a("a"), "af63dc4c8601ec8c");
}

#[test]
fn canonical_input_bounds_unicode_without_splitting_characters() {
    let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
    let event = ev("1")
        .title(&family.repeat(300))
        .location(&family.repeat(600))
        .notes(&family.repeat(600))
        .attendees(
            (0..20)
                .map(|_| named_attendee(Some(&family.repeat(90)), None))
                .collect(),
        )
        .build();
    let info = CalendarInfo::new("fake", "cal", family.repeat(200));
    let projected = AdjudicationEvent::new(&event, Some(&info));
    assert_eq!(grapheme_count(&projected.title), 256);
    assert_eq!(projected.location.as_deref().map(grapheme_count), Some(512));
    assert_eq!(projected.notes.as_deref().map(grapheme_count), Some(500));
    assert_eq!(
        projected.calendar_title.as_deref().map(grapheme_count),
        Some(128)
    );
    assert_eq!(projected.attendee_names.len(), 10);
    assert!(
        projected
            .attendee_names
            .iter()
            .all(|name| grapheme_count(name) == 80 && !name.contains('\u{FFFD}'))
    );
}

#[test]
fn canonical_cache_identity_includes_presented_data_and_policy() {
    let a = AdjudicationEvent::new(&ev("1").title("Doctor").build(), None);
    let mut b = AdjudicationEvent::new(&ev("2").title("Clinic").build(), None);
    let request =
        |b: &AdjudicationEvent| AdjudicationRequest::new("", a.clone(), b.clone(), vec![]);
    let base = request(&b);
    let key = base.input().cache_key_with_policy("apple", 2);
    assert_eq!(base.input().cache_key_with_policy("apple", 2), key);
    assert_ne!(base.input().cache_key_with_policy("other", 2), key);
    assert_ne!(base.input().cache_key_with_policy("apple", 3), key);
    b.calendar_title = Some("Kristin".to_string());
    assert_ne!(request(&b).input().cache_key_with_policy("apple", 2), key);
    b.calendar_title = None;
    b.attendee_names = vec!["Kristin".to_string()];
    assert_ne!(request(&b).input().cache_key_with_policy("apple", 2), key);
}

#[test]
fn cache_key_ignores_text_beyond_rendered_prompt_budget() {
    let mut a = AdjudicationEvent::new(&ev("1").title(&"\\\n".repeat(256)).build(), None);
    let b = AdjudicationEvent::new(&ev("2").title("Clinic").build(), None);
    let first = AdjudicationRequest::new("", a.clone(), b.clone(), vec![])
        .input()
        .cache_key("apple");
    a.title.pop();
    a.title.push('Z');
    let second = AdjudicationRequest::new("", a, b, vec![])
        .input()
        .cache_key("apple");
    assert_eq!(first, second);
}

#[test]
fn equivalent_swapped_pair_and_lesson_usage_keep_cache_identity() {
    let first_event = ev("1").title("Doctor").build();
    let second_event = ev("2").title("Clinic").build();
    let a = AdjudicationEvent::new(&first_event, None);
    let b = AdjudicationEvent::new(&second_event, None);
    let mut book = LessonBook::default();
    book.record(
        &MergedMember::from_event(&first_event),
        &MergedMember::from_event(&second_event),
        LessonDecision::Same,
        t0(),
    );
    let mut forward = AdjudicationRequest::new("", a.clone(), b.clone(), book.lessons().to_vec());
    forward.start_offset_minutes = 10;
    forward.end_offset_minutes = -5;
    forward.first_details = "location".to_string();
    forward.second_details = "bare".to_string();
    let mut touched = book.lessons().to_vec();
    touched[0].last_used = add_seconds(t0(), 100.0);
    let reverse_with = |lessons: Vec<_>| {
        let mut request = AdjudicationRequest::new("", b.clone(), a.clone(), lessons);
        request.start_offset_minutes = -10;
        request.end_offset_minutes = 5;
        request.first_details = "bare".to_string();
        request.second_details = "location".to_string();
        request
    };
    assert_eq!(
        forward.input().cache_key("apple"),
        reverse_with(touched.clone()).input().cache_key("apple")
    );
    touched[0].decision = LessonDecision::Different;
    assert_ne!(
        forward.input().cache_key("apple"),
        reverse_with(touched).input().cache_key("apple")
    );
}

#[test]
fn attendees_without_names_are_not_listed() {
    // `attendee(..)` builds `Attendee(email:)`; adjudication projections never expose the address.
    let event = ev("1").attendees(vec![attendee("k@x.com")]).build();
    assert!(
        AdjudicationEvent::new(&event, None)
            .attendee_names
            .is_empty()
    );
}
