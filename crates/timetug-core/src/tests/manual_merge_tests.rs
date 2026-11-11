//! Port of `TimeTugCoreTests/ManualMergeTests.swift`. `mergingTheReportedPairThroughTheStoreSticks` goes through
//! `CalendarStore` (Task 4) and is ported there.

use super::dedup_support::{ev, make_all_day_in};
use super::support::{date, day};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::resolver::{DuplicateResolution, DuplicateResolver};
use crate::dedup::rules::DuplicateRules;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::{MergeProvenance, MergedMember};

fn t0() -> jiff::Timestamp {
    date("2026-09-18T09:00:00Z")
}

fn resolve(events: &[TimeTugCalendarEvent], lessons: &LessonBook) -> DuplicateResolution {
    DuplicateResolver::resolve(events, &[], lessons, None)
}

fn ids(list: &[TimeTugCalendarEvent]) -> Vec<String> {
    list.iter().map(TimeTugCalendarEvent::id).collect()
}

#[test]
fn manual_candidates_must_actually_overlap() {
    let a = ev("1")
        .title("A")
        .start("2026-09-18T10:00:00Z")
        .minutes(30)
        .cal("x")
        .build();
    let touching = ev("2")
        .title("B")
        .start("2026-09-18T10:30:00Z")
        .minutes(30)
        .cal("x")
        .build();
    let overlapping = ev("3")
        .title("C")
        .start("2026-09-18T10:15:00Z")
        .minutes(60)
        .cal("x")
        .build();
    let far_apart = ev("4")
        .title("D")
        .start("2026-09-18T08:00:00Z")
        .minutes(240)
        .cal("y")
        .build();
    let all_day = make_all_day_in(
        "d",
        "UTC",
        day(2026, 9, 18),
        day(2026, 9, 19),
        "Holiday",
        "y",
    );
    assert!(!DuplicateRules::is_manual_candidate(&a, &touching));
    assert!(DuplicateRules::is_manual_candidate(&a, &overlapping)); // same calendar is fine
    assert!(DuplicateRules::is_manual_candidate(&a, &far_apart)); // starts 2 hours apart is fine
    assert!(!DuplicateRules::is_manual_candidate(&a, &all_day));
}

/// The reported case: a stale 10:00 hold on a subscribed calendar and the real 10:30 consultation, copied onto
/// that calendar and three others. They only touch, so neither row offers "Merge".
#[test]
fn back_to_back_hold_and_consultation_are_not_offered_for_merge() {
    let zoom = "https://careerminds.zoom.us/j/78751303881";
    let hold = ev("h")
        .title("Scott: Careerminds consultation with Michael (tentative)")
        .start("2026-09-18T10:00:00Z")
        .minutes(30)
        .cal("shared")
        .others(0)
        .build();
    let mut events = vec![hold];
    for (index, calendar) in ["shared", "shared2", "shared3", "google"]
        .iter()
        .enumerate()
    {
        events.push(
            ev(&format!("c{index}"))
                .title("Consultation with Michael Munir")
                .start("2026-09-18T10:30:00Z")
                .minutes(30)
                .cal(calendar)
                .notes(&format!("Join {zoom}"))
                .build(),
        );
    }
    let result = resolve(&events, &LessonBook::default());
    assert_eq!(result.events.len(), 2);
    assert!(result.candidates.is_empty());
}

#[test]
fn overlapping_events_on_one_calendar_are_offered_and_a_merge_sticks() {
    let a = ev("1")
        .title("Hold")
        .start("2026-09-18T10:00:00Z")
        .minutes(60)
        .cal("cal")
        .others(0)
        .build();
    let b = ev("2")
        .title("Consultation")
        .start("2026-09-18T10:30:00Z")
        .minutes(30)
        .cal("cal")
        .build();
    let before = resolve(&[a.clone(), b.clone()], &LessonBook::default());
    assert_eq!(before.events.len(), 2);
    assert_eq!(ids(&before.candidates[&a.id()]), [b.id()]);
    assert_eq!(ids(&before.candidates[&b.id()]), [a.id()]);

    let mut lessons = LessonBook::default();
    lessons.record(
        &MergedMember::from_event(&a),
        &MergedMember::from_event(&b),
        LessonDecision::Same,
        t0(),
    );
    let after = resolve(&[a, b], &lessons);
    assert_eq!(after.events.len(), 1);
    assert_eq!(
        after.events[0].merge_provenance,
        Some(MergeProvenance::UserConfirmed)
    );
}

fn monday_tuesday() -> [TimeTugCalendarEvent; 4] {
    [
        ev("1")
            .title("Hold")
            .start("2026-09-14T10:00:00Z")
            .minutes(60)
            .cal("cal")
            .build(),
        ev("2")
            .title("Call")
            .start("2026-09-14T10:30:00Z")
            .minutes(30)
            .cal("cal")
            .build(),
        ev("3")
            .title("Hold")
            .start("2026-09-15T10:00:00Z")
            .minutes(60)
            .cal("cal")
            .build(),
        ev("4")
            .title("Call")
            .start("2026-09-15T10:30:00Z")
            .minutes(30)
            .cal("cal")
            .build(),
    ]
}

#[test]
fn a_same_calendar_lesson_only_applies_to_those_occurrences() {
    let [monday, monday_call, tuesday, tuesday_call] = monday_tuesday();
    let mut lessons = LessonBook::default();
    lessons.record(
        &MergedMember::from_event(&monday),
        &MergedMember::from_event(&monday_call),
        LessonDecision::Same,
        t0(),
    );
    assert_eq!(resolve(&[monday, monday_call], &lessons).events.len(), 1);
    assert_eq!(resolve(&[tuesday, tuesday_call], &lessons).events.len(), 2);
}

#[test]
fn same_calendar_lessons_for_different_days_are_kept_separately() {
    let [monday, monday_call, tuesday, tuesday_call] = monday_tuesday();
    let mut lessons = LessonBook::default();
    lessons.record(
        &MergedMember::from_event(&monday),
        &MergedMember::from_event(&monday_call),
        LessonDecision::Same,
        t0(),
    );
    lessons.record(
        &MergedMember::from_event(&tuesday),
        &MergedMember::from_event(&tuesday_call),
        LessonDecision::Same,
        t0(),
    );
    assert_eq!(lessons.lessons().len(), 2);
}

#[test]
fn overlapping_events_with_far_apart_starts_are_offered_but_not_merged_by_rules() {
    let block = ev("1")
        .title("Workshop")
        .start("2026-09-18T08:00:00Z")
        .minutes(240)
        .cal("x")
        .build();
    let session = ev("2")
        .title("Workshop")
        .start("2026-09-18T10:00:00Z")
        .minutes(30)
        .cal("y")
        .build();
    let result = resolve(&[block.clone(), session.clone()], &LessonBook::default());
    assert_eq!(result.events.len(), 2);
    assert_eq!(ids(&result.candidates[&block.id()]), [session.id()]);

    let mut lessons = LessonBook::default();
    lessons.record(
        &MergedMember::from_event(&block),
        &MergedMember::from_event(&session),
        LessonDecision::Same,
        t0(),
    );
    assert_eq!(resolve(&[block, session], &lessons).events.len(), 1);
}

#[test]
fn lessons_saved_before_occurrence_scoping_still_decode() {
    let json = r#"
    {"lessons":[{"titleA":"a","titleB":"b","calendarKeyA":"fake/x","calendarKeyB":"fake/y","signalsA":"bare",
    "signalsB":"bare","decision":"same","lastUsed":0}]}
    "#;
    let book: LessonBook = serde_json::from_str(json).unwrap();
    assert_eq!(book.lessons().len(), 1);
    assert_eq!(book.lessons()[0].content_key_a, None);
    let a = ev("1").title("A").cal("x").build();
    let b = ev("2").title("B").cal("y").build();
    assert_eq!(
        book.decision(&a, &b).map(|l| l.decision),
        Some(LessonDecision::Same)
    );
}
