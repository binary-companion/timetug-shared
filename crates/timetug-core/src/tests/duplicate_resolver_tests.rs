//! Port of `TimeTugCoreTests/DuplicateResolverTests.swift`.
//!
//! Four Swift tests also assert through `TakeoverPolicy`, `Scheduler` and `TakeoverLedger`, which Task 2 ports in a
//! separate pull request. Those assertions are left out here and marked `Task 2:` in place; the resolver assertions
//! of the same tests are all ported.

use std::collections::BTreeSet;

use calendar_core::model::ResponseStatus;
use calendar_core::time::add_seconds;

use super::dedup_support::{ev, identities};
use super::support::date;
use crate::dedup::adjudication::{AdjudicationVerdict, EngineInfo, VerdictAnswer, VerdictCache};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::resolver::{DuplicateResolution, DuplicateResolver};
use crate::dedup::rules::{DuplicateRules, MergeReason, PairDecision, SeparateReason};
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::{MergeProvenance, MergedMember};

fn t0() -> jiff::Timestamp {
    date("2026-09-18T09:00:00Z")
}

fn engine() -> EngineInfo {
    EngineInfo::new("test", "Test AI", true)
}

fn inference() -> MergeProvenance {
    MergeProvenance::Inference {
        engine_id: "test".to_string(),
        engine_name: "Test AI".to_string(),
    }
}

fn doctor() -> TimeTugCalendarEvent {
    ev("1")
        .title("Scott: Doctor")
        .minutes(60)
        .cal("personal")
        .others(0)
        .build()
}

fn official() -> TimeTugCalendarEvent {
    ev("2")
        .title("Intermountain Health")
        .minutes(60)
        .cal("work")
        .location("1234 Main St, Logan")
        .notes("Bring insurance card")
        .build()
}

fn dance() -> TimeTugCalendarEvent {
    ev("3")
        .title("Kristin: Logan Dance")
        .minutes(60)
        .cal("personal")
        .others(0)
        .build()
}

fn resolve(events: &[TimeTugCalendarEvent]) -> DuplicateResolution {
    DuplicateResolver::resolve(events, &[], &LessonBook::default(), None)
}

fn resolve_with(
    events: &[TimeTugCalendarEvent],
    lessons: &LessonBook,
    verdicts: Option<&VerdictCache>,
) -> DuplicateResolution {
    DuplicateResolver::resolve(events, &[], lessons, verdicts)
}

/// `resolve(events, verdicts: VerdictCache())`: inference on, nothing answered yet.
fn resolve_asking(events: &[TimeTugCalendarEvent]) -> DuplicateResolution {
    resolve_with(
        events,
        &LessonBook::default(),
        Some(&VerdictCache::default()),
    )
}

fn cache_answering(answer: VerdictAnswer, resolution: &DuplicateResolution) -> VerdictCache {
    let mut cache = VerdictCache::default();
    for request in &resolution.pending {
        cache.store(
            &AdjudicationVerdict::new(request.id.clone(), answer),
            &engine(),
            add_seconds(t0(), 86_400.0),
            t0(),
        );
    }
    cache
}

fn member(event: &TimeTugCalendarEvent) -> MergedMember {
    MergedMember::from_event(event)
}

fn keys(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|k| k.to_string()).collect()
}

fn ids(list: &[TimeTugCalendarEvent]) -> Vec<String> {
    list.iter().map(TimeTugCalendarEvent::id).collect()
}

#[test]
fn exact_duplicates_merge_with_rule_provenance_and_keep_the_first_on_a_tie() {
    let a = ev("1").title("Sync").cal("family").build();
    let b = ev("2").title("sync").cal("work").build();
    let result = resolve(&[a, b]);
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].calendar_key(), "fake/family");
    assert_eq!(
        result.events[0].additional_calendar_keys,
        keys(&["fake/work"])
    );
    assert_eq!(
        result.events[0].merge_provenance,
        Some(MergeProvenance::Rule)
    );
    assert_eq!(result.events[0].merged_members.len(), 2);
}

#[test]
fn richer_copy_becomes_primary_and_borrows_missing_details() {
    let bare = ev("1").title("Sync").cal("a").build();
    let rich = ev("2")
        .title("Sync")
        .cal("b")
        .notes("Join https://acme.zoom.us/j/123")
        .build();
    let merged = resolve(&[bare, rich]).events.remove(0);
    assert_eq!(merged.calendar_key(), "fake/b");
    assert_eq!(merged.additional_calendar_keys, keys(&["fake/a"]));
}

#[test]
fn ambiguous_pair_is_pending_when_inference_is_on_and_unknown() {
    let result = resolve_asking(&[doctor(), official()]);
    assert_eq!(result.events.len(), 2);
    assert_eq!(result.pending.len(), 1);
    assert_eq!(result.pending[0].first.title, "Intermountain Health"); // richer first
    assert_eq!(result.pending[0].second.title, "Scott: Doctor");
}

#[test]
fn equivalent_pair_order_and_lesson_touch_keep_the_same_judgment_key() {
    let left = resolve_asking(&[doctor(), official()]).pending.remove(0);
    let swapped = resolve_asking(&[official(), doctor()]).pending.remove(0);
    assert_eq!(left.id, swapped.id);

    let a = MergedMember::new(
        "Doctor parking",
        "fake/third",
        "a",
        "bare",
        t0(),
        add_seconds(t0(), 3600.0),
    );
    let b = MergedMember::new(
        "Parking",
        "fake/fourth",
        "b",
        "bare",
        t0(),
        add_seconds(t0(), 3600.0),
    );
    let mut lessons = LessonBook::default();
    lessons.record(&a, &b, LessonDecision::Same, t0());
    let pending_id = |lessons: &LessonBook| {
        resolve_with(
            &[doctor(), official()],
            lessons,
            Some(&VerdictCache::default()),
        )
        .pending
        .remove(0)
        .id
    };
    let with_lesson = pending_id(&lessons);
    let key = lessons.lessons()[0].pair_key();
    lessons.touch(&BTreeSet::from([key]), add_seconds(t0(), 60.0));
    assert_eq!(pending_id(&lessons), with_lesson);
    lessons.record(&a, &b, LessonDecision::Different, add_seconds(t0(), 120.0));
    assert_ne!(pending_id(&lessons), with_lesson);
}

#[test]
fn a_person_named_calendar_changes_the_judgment_key() {
    let original = resolve_asking(&[doctor(), official()]).pending.remove(0);
    let named = CalendarInfo::new("fake", "personal", "Kristin");
    let updated = DuplicateResolver::resolve(
        &[doctor(), official()],
        &[named],
        &LessonBook::default(),
        Some(&VerdictCache::default()),
    )
    .pending
    .remove(0);
    assert_ne!(original.id, updated.id);
}

#[test]
fn rules_only_never_produces_pending() {
    let result = resolve(&[doctor(), official()]);
    assert_eq!(result.events.len(), 2);
    assert!(result.pending.is_empty());
}

#[test]
fn cached_same_verdict_merges_with_inference_provenance() {
    let verdicts = cache_answering(
        VerdictAnswer::Same,
        &resolve_asking(&[doctor(), official()]),
    );
    let result = resolve_with(
        &[doctor(), official()],
        &LessonBook::default(),
        Some(&verdicts),
    );
    assert_eq!(result.events.len(), 1);
    assert!(result.pending.is_empty());
    let merged = &result.events[0];
    assert_eq!(merged.title(), "Intermountain Health");
    assert_eq!(merged.merge_provenance, Some(inference()));
    assert_eq!(merged.additional_calendar_keys, keys(&["fake/personal"]));
    let mut titles: Vec<&str> = merged
        .merged_members
        .iter()
        .map(|m| m.title.as_str())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Intermountain Health", "Scott: Doctor"]);
}

#[test]
fn unsure_and_different_verdicts_do_not_merge() {
    for answer in [VerdictAnswer::Unsure, VerdictAnswer::Different] {
        let verdicts = cache_answering(answer, &resolve_asking(&[doctor(), official()]));
        let result = resolve_with(
            &[doctor(), official()],
            &LessonBook::default(),
            Some(&verdicts),
        );
        assert_eq!(result.events.len(), 2, "{answer:?}");
        assert!(result.pending.is_empty(), "{answer:?}");
    }
}

#[test]
fn same_calendar_events_at_the_same_time_stay_separate() {
    let result = resolve_asking(&[doctor(), dance()]);
    assert_eq!(result.events.len(), 2);
    assert!(result.pending.is_empty());
}

#[test]
fn lesson_same_merges_even_without_inference_and_is_reported() {
    let mut lessons = LessonBook::default();
    lessons.record(
        &member(&doctor()),
        &member(&official()),
        LessonDecision::Same,
        t0(),
    );
    let result = resolve_with(&[doctor(), official()], &lessons, None);
    assert_eq!(result.events.len(), 1);
    assert_eq!(
        result.events[0].merge_provenance,
        Some(MergeProvenance::UserConfirmed)
    );
    assert_eq!(result.used_lesson_keys.len(), 1);
}

#[test]
fn lesson_different_beats_a_strong_rule_and_offers_a_manual_merge() {
    let zoom = "https://acme.zoom.us/j/9";
    let a = ev("1").title("Weekly").cal("a").location(zoom).build();
    let b = ev("2").title("Team sync").cal("b").location(zoom).build();
    assert_eq!(resolve(&[a.clone(), b.clone()]).events.len(), 1);
    let mut lessons = LessonBook::default();
    lessons.record(&member(&a), &member(&b), LessonDecision::Different, t0());
    let result = resolve_with(&[a.clone(), b.clone()], &lessons, None);
    assert_eq!(result.events.len(), 2);
    assert_eq!(ids(&result.candidates[&a.id()]), [b.id()]);
    assert_eq!(ids(&result.candidates[&b.id()]), [a.id()]);
}

#[test]
fn lesson_same_still_respects_the_time_gate() {
    let mut lessons = LessonBook::default();
    let late = ev("2")
        .title("Intermountain Health")
        .start("2026-09-18T15:00:00Z")
        .minutes(60)
        .cal("work")
        .build();
    lessons.record(
        &member(&doctor()),
        &member(&late),
        LessonDecision::Same,
        t0(),
    );
    assert_eq!(
        resolve_with(&[doctor(), late], &lessons, None).events.len(),
        2
    );
}

#[test]
fn merge_is_skipped_when_any_group_member_blocks_it() {
    let zoom = "https://acme.zoom.us/j/9";
    let a = ev("1").title("One").cal("a").location(zoom).build();
    let b = ev("2").title("Two").cal("b").location(zoom).build();
    let c = ev("3").title("Three").cal("a").location(zoom).build(); // same calendar as a
    assert_eq!(resolve(&[a, b, c]).events.len(), 2);
}

#[test]
fn candidates_list_separate_look_alikes() {
    let (d, o) = (doctor(), official());
    let result = resolve(&[d.clone(), o.clone()]);
    assert_eq!(ids(&result.candidates[&d.id()]), [o.id()]);
    assert_eq!(ids(&result.candidates[&o.id()]), [d.id()]);
}

#[test]
fn equal_start_and_title_events_sort_deterministically_by_id() {
    let a = ev("1").title("Sync").cal("a").build();
    let b = ev("2").title("Sync").cal("b").build();
    let mut lessons = LessonBook::default();
    lessons.record(&member(&a), &member(&b), LessonDecision::Different, t0());
    let forward = ids(&resolve_with(&[a.clone(), b.clone()], &lessons, None).events);
    let backward = ids(&resolve_with(&[b, a], &lessons, None).events);
    assert_eq!(forward.len(), 2);
    assert_eq!(forward, backward);
}

const ZOOM: &str = "https://acme.zoom.us/j/123456";

#[test]
fn merged_event_keeps_the_largest_attendee_count_so_takeover_still_qualifies() {
    let work = ev("1")
        .title("Planning")
        .cal("cal")
        .others(5)
        .conf(ZOOM)
        .build();
    let personal = ev("2")
        .title("Planning copy")
        .cal("personal")
        .others(0)
        .location("Room 4")
        .notes("Agenda")
        .conf(ZOOM)
        .build();
    // Task 2: TakeoverPolicy::qualifies(&work, opted_in()) is true before the merge.
    let merged = resolve(&[work, personal]).events;
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].calendar_key(), "fake/personal"); // the richer copy is the primary
    assert_eq!(merged[0].other_attendee_count, 5);
    // Task 2: TakeoverPolicy::qualifies(&merged[0], opted_in()) is true after it.
}

#[test]
fn merged_event_is_not_declined_when_any_copy_is_not_declined() {
    let work = ev("1")
        .title("Planning")
        .cal("cal")
        .others(3)
        .status(Some(ResponseStatus::Accepted))
        .conf(ZOOM)
        .build();
    let personal = ev("2")
        .title("Planning copy")
        .cal("personal")
        .others(3)
        .status(Some(ResponseStatus::Declined))
        .location("Room 4")
        .notes("Agenda")
        .conf(ZOOM)
        .build();
    // Task 2: TakeoverPolicy::qualifies(&work, opted_in()) is true before the merge.
    let merged = resolve(&[work, personal]).events;
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].calendar_key(), "fake/personal");
    assert_eq!(merged[0].response_status, Some(ResponseStatus::Accepted));
    // Task 2: TakeoverPolicy::qualifies(&merged[0], opted_in()) is true after it.
}

#[test]
fn merged_response_status_prefers_the_most_attending() {
    use ResponseStatus::{Accepted, Declined, NeedsAction, Tentative};
    let status = |list: &[Option<ResponseStatus>]| {
        let events: Vec<TimeTugCalendarEvent> = list
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ev(&i.to_string())
                    .title("Sync")
                    .cal(&format!("c{i}"))
                    .status(*s)
                    .build()
            })
            .collect();
        resolve(&events).events[0].response_status
    };
    assert_eq!(status(&[Some(Declined), None]), None);
    assert_eq!(status(&[None, Some(NeedsAction)]), Some(NeedsAction));
    assert_eq!(
        status(&[Some(NeedsAction), Some(Tentative)]),
        Some(Tentative)
    );
    assert_eq!(
        status(&[Some(Tentative), Some(Accepted), Some(Declined)]),
        Some(Accepted)
    );
    assert_eq!(status(&[Some(Declined), Some(Declined)]), Some(Declined));
}

#[test]
fn five_exact_duplicates_across_calendars_merge_into_one_card() {
    let events: Vec<_> = (0..5)
        .map(|i| {
            ev(&format!("e{i}"))
                .title("Sync")
                .cal(&format!("c{i}"))
                .build()
        })
        .collect();
    let result = resolve(&events).events;
    assert_eq!(result.len(), 1);
    let calendar_keys: Vec<&str> = result[0]
        .merged_members
        .iter()
        .map(|m| m.calendar_key.as_str())
        .collect();
    assert_eq!(
        calendar_keys,
        ["fake/c0", "fake/c1", "fake/c2", "fake/c3", "fake/c4"]
    );
    assert_eq!(result[0].additional_calendar_keys.len(), 4);
    assert_eq!(result[0].merge_provenance, Some(MergeProvenance::Rule));
}

#[test]
fn model_merges_join_at_most_four_clusters_and_the_rest_stay_separate() {
    // Five distinct entities that only the model links: the cap limits clusters, not certain duplicates.
    let titles = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    let events: Vec<_> = titles
        .iter()
        .enumerate()
        .map(|(i, t)| ev(&format!("e{i}")).title(t).cal(&format!("c{i}")).build())
        .collect();
    let verdicts = cache_answering(VerdictAnswer::Same, &resolve_asking(&events));
    let result = resolve_with(&events, &LessonBook::default(), Some(&verdicts)).events;
    assert_eq!(result.len(), 2);
    let group = result
        .iter()
        .find(|e| !e.merged_members.is_empty())
        .unwrap();
    let calendar_keys: Vec<&str> = group
        .merged_members
        .iter()
        .map(|m| m.calendar_key.as_str())
        .collect();
    assert_eq!(calendar_keys, ["fake/c0", "fake/c1", "fake/c2", "fake/c3"]);
    assert_eq!(group.merge_provenance, Some(inference()));
    let single = result.iter().find(|e| e.merged_members.is_empty()).unwrap();
    assert_eq!(single.calendar_key(), "fake/c4");
}

// MARK: the model judges each pair of groups once

const APPT_TITLE: &str = "Mando (X1102)'s Upcoming Appointment";

fn appointment_at(n: usize, location: &str) -> TimeTugCalendarEvent {
    ev(&format!("x{n}"))
        .title(APPT_TITLE)
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal(&format!("X{n}"))
        .location(location)
        .notes("Bring your card")
        .build()
}

fn appointment(n: usize) -> TimeTugCalendarEvent {
    appointment_at(n, "Clinic, 5 Main St")
}

fn spem() -> TimeTugCalendarEvent {
    ev("s")
        .title("Mando Spem Collection")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("S")
        .build()
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, head.clone());
            out.push(tail);
        }
    }
    out
}

fn source_ids(order: &[TimeTugCalendarEvent]) -> Vec<&str> {
    order.iter().map(|e| e.source_event_id()).collect()
}

#[test]
fn spem_and_three_identical_copies_produce_one_request_not_three() {
    let mut all = vec![spem()];
    all.extend((1..=3).map(appointment));
    for order in permutations(&all) {
        let result = resolve_asking(&order);
        assert_eq!(result.pending.len(), 1, "order {:?}", source_ids(&order));
        assert_eq!(result.events.len(), 2);
    }
}

#[test]
fn one_same_verdict_merges_the_whole_group_pair() {
    let mut all = vec![spem()];
    all.extend((1..=3).map(appointment));
    let verdicts = cache_answering(VerdictAnswer::Same, &resolve_asking(&all));
    for order in permutations(&all) {
        let result = resolve_with(&order, &LessonBook::default(), Some(&verdicts));
        assert_eq!(result.events.len(), 1, "order {:?}", source_ids(&order));
        assert!(result.pending.is_empty());
        let Some(card) = result.events.first() else {
            continue;
        };
        assert_eq!(card.merged_members.len(), 4);
        assert_eq!(card.merge_provenance, Some(inference()));
        assert_eq!(
            card.all_calendar_keys(),
            keys(&["fake/S", "fake/X1", "fake/X2", "fake/X3"])
        );
    }
}

#[test]
fn one_different_verdict_keeps_spem_apart_and_the_copies_merged() {
    let mut all = vec![spem()];
    all.extend((1..=3).map(appointment));
    let verdicts = cache_answering(VerdictAnswer::Different, &resolve_asking(&all));
    let result = resolve_with(&all, &LessonBook::default(), Some(&verdicts));
    assert_eq!(result.events.len(), 2);
    assert!(result.pending.is_empty());
    assert!(
        result
            .events
            .iter()
            .any(|e| e.merged_members.len() == 3
                && e.merge_provenance == Some(MergeProvenance::Rule))
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| e.merged_members.is_empty() && e.calendar_key() == "fake/S")
    );
}

#[test]
fn request_id_does_not_depend_on_which_identical_copy_represents() {
    let mut all = vec![spem()];
    all.extend((1..=3).map(appointment));
    let ids: Vec<Vec<String>> = permutations(&all)
        .iter()
        .map(|order| {
            resolve_asking(order)
                .pending
                .into_iter()
                .map(|r| r.id)
                .collect()
        })
        .collect();
    let distinct: BTreeSet<&String> = ids.iter().flatten().collect();
    assert_eq!(distinct.len(), 1);
    // Same content on differently keyed calendars gives the same id.
    let other = ev("z")
        .title(APPT_TITLE)
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("Z9")
        .location("Clinic, 5 Main St")
        .notes("Bring your card")
        .build();
    let pending: Vec<String> = resolve_asking(&[spem(), other])
        .pending
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(pending, ids[0]);
}

#[test]
fn an_edited_event_gets_a_new_request_id() {
    let before: Vec<String> = resolve_asking(&[spem(), appointment(1)])
        .pending
        .into_iter()
        .map(|r| r.id)
        .collect();
    let after: Vec<String> = resolve_asking(&[spem(), appointment_at(1, "Other Clinic, 9 Elm St")])
        .pending
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert!(before.len() == 1 && after.len() == 1);
    assert_ne!(before, after);
}

#[test]
fn a_verdict_on_a_merged_group_pair_leaves_new_neighbours_pending_on_the_next_pass() {
    // Spem + copies merge by "same"; a further unrelated look-alike then needs its own request.
    let mut all = vec![spem()];
    all.extend((1..=2).map(appointment));
    let extra = ev("q")
        .title("Something else")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("Q")
        .build();
    all.push(extra);
    let first = resolve_asking(&all);
    let spem_request = first
        .pending
        .iter()
        .find(|r| {
            BTreeSet::from([r.first.title.as_str(), r.second.title.as_str()])
                == BTreeSet::from([APPT_TITLE, spem().title()])
        })
        .unwrap();
    let mut verdicts = VerdictCache::default();
    verdicts.store(
        &AdjudicationVerdict::new(spem_request.id.clone(), VerdictAnswer::Same),
        &engine(),
        add_seconds(t0(), 86_400.0),
        t0(),
    );
    let second = resolve_with(&all, &LessonBook::default(), Some(&verdicts));
    assert_eq!(second.events.len(), 2);
    assert_eq!(second.pending.len(), 1);
}

#[test]
fn a_lesson_different_is_a_hard_block_that_verdicts_cannot_override() {
    let mut all = vec![spem()];
    all.extend((1..=3).map(appointment));
    let mut lessons = LessonBook::default();
    lessons.record(
        &member(&all[0]),
        &member(&all[3]),
        LessonDecision::Different,
        t0(),
    );
    let pending = resolve_with(&all, &lessons, Some(&VerdictCache::default())).pending;
    assert!(pending.is_empty()); // blocked group pair is never asked about
    let result = resolve_with(&all, &lessons, Some(&VerdictCache::default()));
    assert_eq!(result.events.len(), 2);
    assert!(
        result
            .events
            .iter()
            .any(|e| e.merged_members.len() == 3
                && e.merge_provenance == Some(MergeProvenance::Rule))
    );
}

#[test]
fn six_exact_duplicates_plus_a_placeholder_is_two_clusters_and_merges() {
    let mut all = vec![spem()];
    all.extend((1..=6).map(appointment));
    let verdicts = cache_answering(VerdictAnswer::Same, &resolve_asking(&all));
    let result = resolve_with(&all, &LessonBook::default(), Some(&verdicts)).events;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].merged_members.len(), 7);
    assert_eq!(result[0].additional_calendar_keys.len(), 6);
}

#[test]
fn a_user_confirmed_link_makes_a_three_way_merge_user_confirmed() {
    let a = ev("1").title("Sync").cal("a").build();
    let b = ev("2").title("Sync").cal("b").build();
    let c = ev("3").title("Sync").cal("c").build();
    let mut lessons = LessonBook::default();
    lessons.record(&member(&a), &member(&b), LessonDecision::Same, t0());
    let result = resolve_with(&[a, b, c], &lessons, None).events;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].merged_members.len(), 3);
    assert_eq!(
        result[0].merge_provenance,
        Some(MergeProvenance::UserConfirmed)
    );
}

// MARK: conference link survives a merge

fn merged_event(events: &[TimeTugCalendarEvent]) -> TimeTugCalendarEvent {
    let mut result = resolve(events).events;
    assert_eq!(result.len(), 1);
    result.remove(0)
}

#[test]
fn merge_keeps_a_link_found_only_in_the_non_primary_copys_notes() {
    let personal = ev("1")
        .title("Review")
        .cal("personal")
        .others(1)
        .location("Home office")
        .notes("prep slides")
        .uid("u1")
        .build();
    let work = ev("2")
        .title("Review")
        .cal("cal")
        .others(4)
        .notes("Join https://teams.microsoft.com/l/meetup-join/abc")
        .uid("u1")
        .build();
    let merged = merged_event(&[personal, work]);
    assert_eq!(merged.notes(), Some("prep slides")); // personal is the primary
    assert_eq!(
        merged.conference_url().and_then(|u| u.host()).as_deref(),
        Some("teams.microsoft.com")
    );
    // Task 2: TakeoverPolicy::qualifies(&merged, opted_in(require_conference_link)) is true.
}

#[test]
fn a_generic_url_on_the_primary_never_beats_a_provider_link_on_another_copy() {
    let primary = ev("1")
        .title("Review")
        .cal("personal")
        .others(1)
        .location("Home office")
        .notes("prep")
        .url("https://example.com/agenda")
        .uid("u1")
        .build();
    let other = ev("2")
        .title("Review")
        .cal("work")
        .others(1)
        .notes("https://acme.zoom.us/j/123")
        .uid("u1")
        .build();
    let merged = merged_event(&[primary, other]);
    assert_eq!(
        merged.conference_url().and_then(|u| u.host()).as_deref(),
        Some("acme.zoom.us")
    );
}

#[test]
fn the_structured_link_on_any_copy_counts_and_the_primary_wins_ties() {
    let a = ev("1")
        .title("Review")
        .cal("personal")
        .notes("x")
        .uid("u1")
        .build();
    let b = ev("2")
        .title("Review")
        .cal("work")
        .conf("https://chime.aws/222")
        .uid("u1")
        .build();
    assert_eq!(
        merged_event(&[a, b])
            .conference_url()
            .map(|u| u.as_str().to_string()),
        Some("https://chime.aws/222".to_string())
    );
    let c = ev("3")
        .title("Review")
        .cal("x")
        .notes("https://acme.zoom.us/j/1 more")
        .uid("u1")
        .build();
    let d = ev("4")
        .title("Review")
        .cal("y")
        .notes("https://acme.zoom.us/j/2")
        .uid("u1")
        .build();
    assert_eq!(
        merged_event(&[c, d])
            .conference_url()
            .map(|u| u.as_str().to_string()),
        Some("https://acme.zoom.us/j/1".to_string())
    );
}

#[test]
fn unmerged_events_keep_their_own_conference_links_and_the_resolver_detects_none_itself() {
    // The connector's list is used as it is: make_event runs the detector like a connector, a bare event has no link.
    let plain = ev("1")
        .title("Solo")
        .notes("https://acme.zoom.us/j/9")
        .build();
    assert_eq!(
        resolve(std::slice::from_ref(&plain)).events[0]
            .conference_url()
            .and_then(|u| u.host())
            .as_deref(),
        Some("acme.zoom.us")
    );
    let mut bare = ev("3")
        .title("Bare")
        .start("2026-09-18T17:00:00Z")
        .notes("https://acme.zoom.us/j/9")
        .build();
    bare.conferences = vec![];
    assert!(resolve(&[bare]).events[0].conference_url().is_none());
    let structured = ev("2")
        .title("Other")
        .start("2026-09-18T15:00:00Z")
        .conf("https://chime.aws/1")
        .build();
    assert_eq!(
        resolve(&[structured]).events[0]
            .conference_url()
            .map(|u| u.as_str().to_string()),
        Some("https://chime.aws/1".to_string())
    );
}

// MARK: - Three literal phases

#[test]
fn resolve_merges_exact_copies_before_other_rules_and_is_order_independent() {
    // Three identical copies; only one carries the conference link that ties in D, and D's location
    // conflicts with another copy. Exact copies must join first, so D stays out for every input order.
    let a = ev("a")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .cal("a")
        .notes("https://acme.zoom.us/j/555")
        .build();
    let b = ev("b")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .cal("b")
        .location("Room 1")
        .build();
    let c = ev("c")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .cal("c")
        .build();
    let d = ev("d")
        .title("Sync call")
        .start("2026-09-18T13:25:00Z")
        .cal("d")
        .location("Room 9")
        .notes("https://acme.zoom.us/j/555")
        .build();
    let placeholder = ev("p")
        .title("Team Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("e")
        .others(0)
        .build();
    let base = vec![
        a.clone(),
        b.clone(),
        c.clone(),
        d.clone(),
        placeholder.clone(),
    ];
    let verdicts = cache_answering(VerdictAnswer::Same, &resolve_asking(&base));

    let member_sets = |events: &[TimeTugCalendarEvent]| -> BTreeSet<BTreeSet<String>> {
        let result = resolve_with(events, &LessonBook::default(), Some(&verdicts));
        result
            .events
            .iter()
            .map(|e| {
                if e.merged_members.is_empty() {
                    BTreeSet::from([e.calendar_key()])
                } else {
                    e.merged_members
                        .iter()
                        .map(|m| m.calendar_key.clone())
                        .collect()
                }
            })
            .collect()
    };
    let expected: BTreeSet<BTreeSet<String>> = BTreeSet::from([
        keys(&["fake/a", "fake/b", "fake/c", "fake/e"]),
        keys(&["fake/d"]),
    ]);
    let mut reversed = base.clone();
    reversed.reverse();
    let orders = vec![
        base,
        reversed,
        vec![
            d.clone(),
            c.clone(),
            b.clone(),
            a.clone(),
            placeholder.clone(),
        ],
        vec![
            placeholder.clone(),
            d.clone(),
            a.clone(),
            c.clone(),
            b.clone(),
        ],
        vec![
            c.clone(),
            placeholder.clone(),
            a.clone(),
            d.clone(),
            b.clone(),
        ],
        vec![b, d, placeholder, c, a],
    ];
    for order in orders {
        assert_eq!(
            member_sets(&order),
            expected,
            "order {:?}",
            source_ids(&order)
        );
    }
}

// MARK: - Display span (longer copy) and tug time (conference copy)

const ZOOM_LINK: &str = "https://acme.zoom.us/j/777";
const TEAMS_LINK: &str = "https://teams.microsoft.com/l/meetup-join/abc";

#[test]
fn merged_event_displays_the_longer_copy_but_tugs_at_the_carrier_start() {
    let short = ev("1")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("a")
        .conf(ZOOM_LINK)
        .build();
    let long = ev("2")
        .title("Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("b")
        .conf(ZOOM_LINK)
        .build();
    let merged = resolve(&[short, long]).events.remove(0);
    assert_eq!(merged.merged_members.len(), 2);
    assert_eq!(merged.start(), date("2026-09-18T13:00:00Z")); // primary (earliest on a tie) carries the link
    assert_eq!(merged.display_start, Some(date("2026-09-18T12:45:00Z")));
    assert_eq!(merged.shown_start(), date("2026-09-18T12:45:00Z"));
    assert_eq!(merged.end(), date("2026-09-18T13:45:00Z"));
}

#[test]
fn equal_durations_keep_the_primary_as_the_longer_copy() {
    let a = ev("1")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("a")
        .conf(ZOOM_LINK)
        .build();
    let b = ev("2")
        .title("Sync")
        .start("2026-09-18T13:05:00Z")
        .minutes(30)
        .cal("b")
        .conf(ZOOM_LINK)
        .build();
    let merged = resolve(&[a, b]).events.remove(0);
    assert_eq!(merged.start(), date("2026-09-18T13:00:00Z"));
    assert_eq!(merged.end(), date("2026-09-18T13:30:00Z"));
    assert_eq!(merged.display_start, None);
    assert_eq!(merged.shown_start(), merged.start());
}

#[test]
fn unmerged_event_is_unchanged() {
    let event = ev("1")
        .start("2026-09-18T13:00:00Z")
        .conf(ZOOM_LINK)
        .build();
    let result = resolve(std::slice::from_ref(&event)).events.remove(0);
    assert_eq!(result, event);
    assert_eq!(result.display_start, None);
}

#[test]
fn only_a_non_primary_copy_has_the_link_so_it_carries_the_tug() {
    let rich = ev("1")
        .title("Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("a")
        .location("Room 1")
        .notes("Agenda")
        .uid("u")
        .build();
    let linked = ev("2")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("b")
        .conf(ZOOM_LINK)
        .uid("u")
        .build();
    let merged = resolve(&[rich, linked]).events.remove(0);
    assert_eq!(merged.calendar_key(), "fake/a");
    assert_eq!(merged.start(), date("2026-09-18T13:00:00Z"));
    assert_eq!(merged.display_start, Some(date("2026-09-18T12:45:00Z")));
    assert_eq!(merged.end(), date("2026-09-18T13:45:00Z"));
}

#[test]
fn two_non_primary_carriers_tug_at_the_earliest_one() {
    let rich = ev("1")
        .title("Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("a")
        .location("Room 1")
        .notes("Agenda")
        .uid("u")
        .build();
    let late = ev("2")
        .title("Sync")
        .start("2026-09-18T13:10:00Z")
        .minutes(20)
        .cal("b")
        .conf(ZOOM_LINK)
        .uid("u")
        .build();
    let early = ev("3")
        .title("Sync")
        .start("2026-09-18T13:05:00Z")
        .minutes(25)
        .cal("c")
        .conf(TEAMS_LINK)
        .uid("u")
        .build();
    let merged = resolve(&[rich, late, early]).events.remove(0);
    assert_eq!(merged.merged_members.len(), 3);
    assert_eq!(merged.start(), date("2026-09-18T13:05:00Z"));
    assert_eq!(merged.display_start, Some(date("2026-09-18T12:45:00Z")));
}

#[test]
fn no_link_tugs_at_the_longer_copy_start() {
    let short = ev("1")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("a")
        .uid("u")
        .build();
    let long = ev("2")
        .title("Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("b")
        .uid("u")
        .build();
    let merged = resolve(&[short, long]).events.remove(0);
    assert_eq!(merged.start(), date("2026-09-18T12:45:00Z"));
    assert_eq!(merged.display_start, None);
    assert_eq!(merged.end(), date("2026-09-18T13:45:00Z"));
}

#[test]
fn bare_placeholder_with_three_identical_copies_shows_the_placeholder_range() {
    let placeholder = ev("p")
        .title("Team Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("p")
        .others(0)
        .build();
    let mut events = vec![placeholder];
    for id in ["a", "b", "c"] {
        events.push(
            ev(id)
                .title("Sync")
                .start("2026-09-18T13:00:00Z")
                .cal(id)
                .location("Room 1")
                .build(),
        );
    }
    let verdicts = cache_answering(VerdictAnswer::Same, &resolve_asking(&events));
    let merged = resolve_with(&events, &LessonBook::default(), Some(&verdicts)).events;
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].merged_members.len(), 4);
    assert_eq!(merged[0].start(), date("2026-09-18T12:45:00Z"));
    assert_eq!(merged[0].end(), date("2026-09-18T13:45:00Z"));
    assert_eq!(merged[0].title(), "Sync");
    assert_eq!(merged[0].location(), Some("Room 1"));
    assert_eq!(merged[0].display_start, None);
}

#[test]
fn conference_appointment_tugs_at_its_start_while_the_list_shows_the_placeholder_range() {
    let placeholder = ev("p")
        .title("Team Sync")
        .start("2026-09-18T12:30:00Z")
        .minutes(60)
        .cal("other")
        .others(0)
        .build();
    let appointment = ev("a")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("cal")
        .notes("Join https://acme.zoom.us/j/777")
        .build();
    let events = vec![placeholder.clone(), appointment.clone()];
    let verdicts = cache_answering(VerdictAnswer::Same, &resolve_asking(&events));
    let merged = resolve_with(&events, &LessonBook::default(), Some(&verdicts)).events;
    assert_eq!(merged.len(), 1);
    let event = &merged[0];
    assert_eq!(event.display_start, Some(date("2026-09-18T12:30:00Z")));
    assert_eq!(event.start(), date("2026-09-18T13:00:00Z"));
    assert_eq!(event.end(), date("2026-09-18T13:30:00Z"));
    assert!(event.all_content_keys().is_superset(&BTreeSet::from([
        placeholder.content_key(),
        appointment.content_key()
    ])));
    // Task 2: Scheduler::next(merged, lead time 120, empty ledger, now 12:00) fires at 12:58:00 (the tug start, never
    // the display start), and a ledger that marked `appointment` as fired reports the merged card as fired.
}

#[test]
fn merged_no_link_event_fires_at_the_longer_copy_start() {
    let short = ev("1")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("cal")
        .uid("u")
        .build();
    let long = ev("2")
        .title("Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("other")
        .uid("u")
        .build();
    let merged = resolve(&[short, long]).events;
    // The merged event starts at the longer copy's start, 12:45; Task 2 adds the Scheduler assertion (fire 12:43).
    assert_eq!(merged[0].start(), date("2026-09-18T12:45:00Z"));
}

#[test]
fn request_carries_rule_computed_facts() {
    // doctor 10:00-11:00 (bare) vs official 10:00-11:00 (location+notes): richer copy is first.
    let a = ev("1")
        .title("Scott: Doctor")
        .start("2026-09-18T10:00:00Z")
        .minutes(60)
        .cal("p")
        .others(0)
        .build();
    let b = ev("2")
        .title("Intermountain")
        .start("2026-09-18T10:15:00Z")
        .minutes(30)
        .cal("w")
        .others(0)
        .location("1234 Main St")
        .notes("Bring card")
        .build();
    let request = resolve_asking(&[a, b]).pending.remove(0);
    assert!(request.first.title == "Intermountain" && request.second.title == "Scott: Doctor");
    assert_eq!(request.start_offset_minutes, -15); // second (10:00) minus first (10:15)
    assert_eq!(request.end_offset_minutes, 15); // 11:00 minus 10:45
    assert_eq!(request.overlap_minutes, 30);
    assert_eq!(request.first_details, "location+notes");
    assert_eq!(request.second_details, "bare");
    assert!(!request.has_conflicting_details);
}

#[test]
fn merged_members_carry_each_copys_own_times() {
    let short = ev("1")
        .title("Sync")
        .start("2026-09-18T13:00:00Z")
        .minutes(30)
        .cal("a")
        .build();
    let placeholder = ev("2")
        .title("Sync")
        .start("2026-09-18T12:45:00Z")
        .minutes(60)
        .cal("b")
        .others(0)
        .build();
    let mut book = LessonBook::default();
    book.record(
        &member(&short),
        &member(&placeholder),
        LessonDecision::Same,
        t0(),
    );
    let merged = resolve_with(&[short, placeholder], &book, None)
        .events
        .remove(0);
    let by_calendar = |key: &str| {
        merged
            .merged_members
            .iter()
            .find(|m| m.calendar_key == key)
            .unwrap()
    };
    assert_eq!(by_calendar("fake/a").start, date("2026-09-18T13:00:00Z"));
    assert_eq!(by_calendar("fake/a").end, date("2026-09-18T13:30:00Z"));
    assert_eq!(by_calendar("fake/b").start, date("2026-09-18T12:45:00Z"));
    assert_eq!(by_calendar("fake/b").end, date("2026-09-18T13:45:00Z"));
}

#[test]
fn same_title_copies_merge_by_rule_and_show_the_longer_times() {
    let short = ev("1")
        .title("Dance Party")
        .start("2026-09-18T19:15:00Z")
        .minutes(45)
        .cal("shared")
        .build();
    let long = ev("2")
        .title("Dance Party")
        .start("2026-09-18T19:15:00Z")
        .minutes(60)
        .cal("blackout")
        .build();
    let result = resolve(&[short.clone(), long.clone()]);
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].end(), long.end());
    assert_eq!(result.events[0].start(), short.start());
    assert_eq!(
        result.events[0].merge_provenance,
        Some(MergeProvenance::Rule)
    );
    assert_eq!(result.events[0].merged_members.len(), 2);
}

#[test]
fn a_user_same_overrides_a_rules_block_between_other_members_but_not_a_learned_different() {
    let zoom = "https://acme.zoom.us/j/1";
    let a = ev("1")
        .title("Consultation")
        .cal("personal")
        .notes("n")
        .conf(zoom)
        .build();
    let b = ev("2")
        .title("Consultation")
        .cal("shared")
        .notes("n")
        .conf(zoom)
        .build();
    let placeholder = ev("3")
        .title("Scott: Careerminds")
        .cal("shared")
        .others(0)
        .build();
    let mut lessons = LessonBook::default();
    lessons.record(
        &member(&a),
        &member(&placeholder),
        LessonDecision::Same,
        t0(),
    );
    // b and the placeholder share a calendar (a rules "separate"), which must not veto the user's merge.
    assert_eq!(
        resolve_with(&[a.clone(), b.clone(), placeholder], &lessons, None)
            .events
            .len(),
        1
    );
    // A learned "different" between any two members still does.
    lessons.record(
        &member(&b),
        &member(&ev("4").title("Scott: Careerminds").cal("other").build()),
        LessonDecision::Different,
        t0(),
    );
    let other = ev("4")
        .title("Scott: Careerminds")
        .cal("other")
        .others(0)
        .build();
    lessons.record(&member(&other), &member(&a), LessonDecision::Same, t0());
    assert_eq!(resolve_with(&[a, b, other], &lessons, None).events.len(), 2);
}

#[test]
fn copies_sharing_a_second_ranked_link_merge() {
    let a = ev("1")
        .title("Review")
        .cal("a")
        .location("https://acme.zoom.us/j/1")
        .notes("or https://meet.google.com/aaa-bbbb-ccc")
        .build();
    let b = ev("2")
        .title("Sync")
        .cal("b")
        .notes("https://meet.google.com/aaa-bbbb-ccc")
        .build();
    assert_eq!(
        DuplicateRules::decide(&a, &b),
        PairDecision::Merge(MergeReason::ConferenceLink)
    );
}

#[test]
fn lists_with_no_shared_link_veto() {
    let a = ev("1")
        .title("Review")
        .cal("a")
        .location("https://acme.zoom.us/j/1")
        .notes("https://meet.google.com/aaa-bbbb-ccc")
        .build();
    let b = ev("2")
        .title("Sync")
        .cal("b")
        .notes("https://acme.webex.com/meet/x")
        .build();
    assert_eq!(
        DuplicateRules::decide(&a, &b),
        PairDecision::Separate(SeparateReason::ConflictingConference)
    );
}

#[test]
fn the_merged_link_list_is_deduped_with_recognised_providers_first() {
    let a = ev("1")
        .title("Review")
        .cal("a")
        .others(1)
        .notes("x")
        .conf("https://chime.aws/1")
        .uid("u1")
        .build();
    let b = ev("2")
        .title("Review")
        .cal("b")
        .notes("https://acme.zoom.us/j/1 and https://chime.aws/1")
        .uid("u1")
        .build();
    let merged = merged_event(&[a, b]);
    assert_eq!(
        identities(&merged.conferences),
        ["acme.zoom.us/j/1", "chime.aws/1"]
    );
}

#[test]
fn the_tug_time_follows_the_copy_that_supplies_the_link_the_card_opens() {
    // The primary (richest) copy has only the generic event url; the other copy carries a real Meet link and starts later.
    let primary = ev("1")
        .title("Review")
        .start("2026-09-18T10:00:00Z")
        .cal("a")
        .others(1)
        .location("Home office")
        .notes("prep")
        .url("https://example.com/agenda")
        .uid("u1")
        .build();
    let other = ev("2")
        .title("Review")
        .start("2026-09-18T10:20:00Z")
        .cal("b")
        .others(1)
        .conf("https://meet.google.com/aaa-bbbb-ccc")
        .uid("u1")
        .build();
    let merged = merged_event(&[primary, other]);
    assert_eq!(
        merged.conference_url().and_then(|u| u.host()).as_deref(),
        Some("meet.google.com")
    );
    assert_eq!(merged.start(), date("2026-09-18T10:20:00Z"));
}
