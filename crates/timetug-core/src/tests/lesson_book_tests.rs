//! Port of `TimeTugCoreTests/LessonBookTests.swift`.

use std::collections::BTreeSet;

use calendar_core::time::add_seconds;

use super::dedup_support::{at, ev};
use super::support::date;
use crate::dedup::lesson_book::{Lesson, LessonBook, LessonDecision};
use crate::model::merge_types::MergedMember;

fn t0() -> jiff::Timestamp {
    date("2026-09-18T09:00:00Z")
}

fn member(title: &str, calendar: &str, details: &str) -> MergedMember {
    MergedMember::new(title, calendar, title, details, at(0), at(3600))
}

fn bare(title: &str, calendar: &str) -> MergedMember {
    member(title, calendar, "bare")
}

#[test]
fn lesson_is_looked_up_in_either_order_by_normalized_titles() {
    let mut book = LessonBook::default();
    book.record(
        &bare("Scott: Doctor", "fake/personal"),
        &member("Intermountain Health", "fake/work", "location"),
        LessonDecision::Same,
        t0(),
    );
    let a = ev("1").title("scott doctor!").cal("personal").build();
    let b = ev("2").title("INTERMOUNTAIN HEALTH").cal("work").build();
    assert_eq!(
        book.decision(&a, &b).map(|l| l.decision),
        Some(LessonDecision::Same)
    );
    assert_eq!(
        book.decision(&b, &a).map(|l| l.decision),
        Some(LessonDecision::Same)
    );
}

#[test]
fn same_calendar_pairs_are_recorded_scoped_to_their_occurrences_and_exact_duplicates_by_title() {
    let mut book = LessonBook::default();
    book.record(
        &bare("A", "fake/cal"),
        &bare("B", "fake/cal"),
        LessonDecision::Different,
        t0(),
    );
    assert_eq!(book.lessons().len(), 1);
    assert_eq!(book.lessons()[0].content_key_a.as_deref(), Some("A"));
    assert_eq!(book.lessons()[0].content_key_b.as_deref(), Some("B"));
    book.record(
        &bare("A", "fake/cal"),
        &bare("A", "fake/cal"),
        LessonDecision::Different,
        t0(),
    );
    assert_eq!(book.lessons().len(), 2);
    // Identical copies: a title-level lesson, as before.
    assert_eq!(book.lessons().last().unwrap().content_key_a, None);
}

#[test]
fn new_decision_replaces_the_old_one_for_the_pair() {
    let mut book = LessonBook::default();
    let a = bare("A", "fake/x");
    let b = bare("B", "fake/y");
    book.record(&a, &b, LessonDecision::Same, t0());
    book.record(&b, &a, LessonDecision::Different, add_seconds(t0(), 60.0));
    assert_eq!(book.lessons().len(), 1);
    assert_eq!(book.lessons()[0].decision, LessonDecision::Different);
}

#[test]
fn lessons_are_capped_oldest_first() {
    let mut book = LessonBook::default();
    for i in 0..(LessonBook::MAX_LESSONS + 5) {
        book.record(
            &bare(&format!("title{i}"), "fake/x"),
            &bare(&format!("other{i}"), "fake/y"),
            LessonDecision::Same,
            add_seconds(t0(), i as f64),
        );
    }
    assert_eq!(book.lessons().len(), LessonBook::MAX_LESSONS);
    assert!(
        !book
            .lessons()
            .iter()
            .any(|l| l.title_a == "title0" || l.title_b == "title0")
    );
}

#[test]
fn unused_lessons_expire_and_touch_refreshes() {
    let mut book = LessonBook::default();
    book.record(
        &bare("A", "fake/x"),
        &bare("B", "fake/y"),
        LessonDecision::Same,
        t0(),
    );
    book.record(
        &bare("C", "fake/x"),
        &bare("D", "fake/y"),
        LessonDecision::Same,
        t0(),
    );
    let key = book
        .lessons()
        .iter()
        .find(|l| l.title_a == "a")
        .unwrap()
        .pair_key();
    let later = add_seconds(t0(), LessonBook::EXPIRY - 10.0);
    book.touch(&BTreeSet::from([key]), later);
    book.prune(add_seconds(t0(), LessonBook::EXPIRY + 10.0));
    let titles: Vec<&str> = book.lessons().iter().map(|l| l.title_a.as_str()).collect();
    assert_eq!(titles, ["a"]);
}

#[test]
fn relevant_lessons_prefer_shared_words_and_calendars_over_recency() {
    let mut book = LessonBook::default();
    book.record(
        &bare("Doctor visit", "fake/personal"),
        &bare("Clinic", "fake/work"),
        LessonDecision::Same,
        t0(),
    );
    book.record(
        &bare("Dance", "fake/personal"),
        &bare("Studio", "fake/work"),
        LessonDecision::Different,
        add_seconds(t0(), 500.0),
    );
    let a = ev("1").title("Doctor").cal("personal").build();
    let b = ev("2").title("Mercy Clinic").cal("work").build();
    assert_eq!(book.relevant(&a, &b)[0].title_a, "clinic");
}

#[test]
fn codable_round_trip() {
    let mut book = LessonBook::default();
    book.record(
        &bare("A", "fake/x"),
        &bare("B", "fake/y"),
        LessonDecision::Same,
        t0(),
    );
    let decoded: LessonBook = serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
    assert_eq!(decoded, book);
}

#[test]
fn relevant_returns_at_most_prompt_limit_and_excludes_unrelated_lessons() {
    let mut book = LessonBook::default();
    for i in 0..8 {
        book.record(
            &bare(&format!("Doctor {i}"), &format!("fake/p{i}")),
            &bare(&format!("Clinic {i}"), &format!("fake/w{i}")),
            LessonDecision::Same,
            add_seconds(t0(), i as f64),
        );
    }
    book.record(
        &bare("Dance", "fake/x"),
        &bare("Studio", "fake/y"),
        LessonDecision::Different,
        t0(),
    );
    let a = ev("1").title("Doctor").cal("personal").build();
    let b = ev("2").title("Clinic").cal("work").build();
    let relevant = book.relevant(&a, &b);
    assert_eq!(relevant.len(), LessonBook::PROMPT_LIMIT);
    assert!(!relevant.iter().any(|l: &Lesson| l.title_a == "dance"));
    let unrelated = ev("3").title("Zzz").cal("personal").build();
    assert!(
        book.relevant(&unrelated, &ev("4").title("Yyy").cal("work").build())
            .is_empty()
    );
}
