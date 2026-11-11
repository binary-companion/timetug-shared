//! Port of `TimeTugCoreTests/DuplicateResolverEquivalenceTests.swift`.

use calendar_core::time::add_seconds;

use super::baseline_duplicate_resolver::BaselineDuplicateResolver;
use super::dedup_support::{ev, make_all_day_in};
use super::support::{date, day};
use crate::dedup::adjudication::{AdjudicationVerdict, EngineInfo, VerdictAnswer, VerdictCache};
use crate::dedup::lesson_book::{LessonBook, LessonDecision};
use crate::dedup::resolver::DuplicateResolver;
use crate::model::event::TimeTugCalendarEvent as Event;
use crate::model::merge_types::MergedMember;

fn equivalence_now() -> jiff::Timestamp {
    date("2026-09-18T09:00:00Z")
}

fn check_equivalent(events: &[Event], lessons: &LessonBook) {
    for inference_on in [false, true] {
        let empty = inference_on.then(VerdictCache::default);
        let baseline = BaselineDuplicateResolver::resolve(events, &[], lessons, empty.as_ref());
        let actual = DuplicateResolver::resolve(events, &[], lessons, empty.as_ref());
        assert_eq!(actual.events, baseline.events);
        assert_eq!(actual.pending, baseline.pending);
        assert_eq!(actual.candidates, baseline.candidates);
        assert_eq!(actual.used_lesson_keys, baseline.used_lesson_keys);

        if !inference_on {
            continue;
        }
        let mut verdicts = VerdictCache::default();
        for (index, request) in baseline.pending.iter().enumerate() {
            let answer = if index % 3 == 0 {
                VerdictAnswer::Same
            } else if index % 2 == 0 {
                VerdictAnswer::Different
            } else {
                VerdictAnswer::Unsure
            };
            verdicts.store(
                &AdjudicationVerdict::new(request.id.clone(), answer),
                &EngineInfo::new("equivalence", "Equivalence model", true),
                add_seconds(equivalence_now(), 86_400.0),
                equivalence_now(),
            );
        }
        let decided_baseline =
            BaselineDuplicateResolver::resolve(events, &[], lessons, Some(&verdicts));
        let decided_actual = DuplicateResolver::resolve(events, &[], lessons, Some(&verdicts));
        assert_eq!(decided_actual.events, decided_baseline.events);
        assert_eq!(decided_actual.pending, decided_baseline.pending);
        assert_eq!(decided_actual.candidates, decided_baseline.candidates);
        assert_eq!(
            decided_actual.used_lesson_keys,
            decided_baseline.used_lesson_keys
        );
    }
}

#[test]
fn resolver_matches_frozen_oracle_on_boundary_and_lesson_fixtures() {
    let zoom = "https://acme.zoom.us/j/123";
    let teams = "https://teams.microsoft.com/l/meetup-join/other";
    use calendar_core::model::UidScope::Global;
    let events: Vec<Event> = vec![
        ev("exact-a").title("Shared").cal("a").build(),
        ev("exact-b").title("Shared").cal("b").build(),
        ev("same-cal").title("Shared").cal("a").build(),
        make_all_day_in(
            "day-a",
            "UTC",
            day(2026, 9, 18),
            day(2026, 9, 19),
            "Holiday",
            "a",
        ),
        make_all_day_in(
            "day-b",
            "UTC",
            day(2026, 9, 18),
            day(2026, 9, 19),
            "Holiday",
            "b",
        ),
        ev("uid-a")
            .title("UID one")
            .cal("c")
            .uid("common")
            .scope(Global)
            .build(),
        ev("uid-b")
            .title("UID two")
            .cal("d")
            .uid("common")
            .scope(Global)
            .build(),
        ev("room-a")
            .title("Planning")
            .cal("e")
            .location("Room 101")
            .build(),
        ev("room-b")
            .title("Planning")
            .cal("f")
            .location("Room 202")
            .build(),
        ev("link-a").title("Call A").cal("g").conf(zoom).build(),
        ev("link-b").title("Call B").cal("h").conf(teams).build(),
        ev("early")
            .title("Early")
            .start("2026-09-18T08:00:00Z")
            .minutes(30)
            .cal("i")
            .build(),
        ev("late")
            .title("Late")
            .start("2026-09-18T14:00:00Z")
            .minutes(30)
            .cal("j")
            .build(),
    ];
    let mut lessons = LessonBook::default();
    lessons.record(
        &MergedMember::from_event(&events[0]),
        &MergedMember::from_event(&events[1]),
        LessonDecision::Different,
        equivalence_now(),
    );
    lessons.record(
        &MergedMember::from_event(&events[7]),
        &MergedMember::from_event(&events[8]),
        LessonDecision::Same,
        equivalence_now(),
    );
    check_equivalent(&events, &lessons);
}

#[test]
fn resolver_matches_frozen_oracle_for_deterministic_randomized_inputs() {
    let mut state: u64 = 0x5eed_2026;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        state
    };
    let starts = [
        "2026-09-18T08:00:00Z",
        "2026-09-18T08:20:00Z",
        "2026-09-18T08:45:00Z",
        "2026-09-18T09:00:00Z",
        "2026-09-18T09:30:00Z",
        "2026-09-18T11:00:00Z",
    ];
    let titles = ["Sync", "Planning", "Standup", "Review", "Holiday"];
    let locations: [Option<&str>; 4] =
        [None, Some("Room 101"), Some("Room 202"), Some("Main hall")];
    for fixture in 0..16 {
        let mut events: Vec<Event> = Vec::new();
        for index in 0..24 {
            let start = starts[(next() % starts.len() as u64) as usize];
            let title = titles[(next() % titles.len() as u64) as usize];
            let calendar = format!("c{}", next() % 5);
            let location = locations[(next() % locations.len() as u64) as usize];
            let uid = if next() % 7 == 0 {
                Some(format!("uid-{}", next() % 4))
            } else {
                None
            };
            let minutes = [30, 60, 90][(next() % 3) as usize];
            let mut builder = ev(&format!("f{fixture}-e{index}"))
                .title(title)
                .start(start)
                .minutes(minutes)
                .cal(&calendar);
            if let Some(location) = location {
                builder = builder.location(location);
            }
            if let Some(uid) = &uid {
                builder = builder
                    .uid(uid)
                    .scope(calendar_core::model::UidScope::Global);
            }
            events.push(builder.build());
        }
        check_equivalent(&events, &LessonBook::default());
    }
}

#[test]
fn resolver_keeps_a_separation_across_a_chain_of_overlapping_events() {
    // The first and last events do not overlap. A middle copy can match each one,
    // but the original resolver treats that distant pair as a hard block.
    let events: Vec<Event> = vec![
        ev("chain-a")
            .title("Standup")
            .start("2026-09-18T09:00:00Z")
            .minutes(30)
            .cal("a")
            .build(),
        ev("chain-b")
            .title("Standup")
            .start("2026-09-18T09:20:00Z")
            .minutes(30)
            .cal("b")
            .build(),
        ev("chain-c")
            .title("Standup")
            .start("2026-09-18T09:40:00Z")
            .minutes(30)
            .cal("c")
            .build(),
    ];
    check_equivalent(&events, &LessonBook::default());
}

#[test]
fn resolver_preserves_candidate_deduplication_when_output_ids_collide() {
    let events: Vec<Event> = vec![
        ev("same-id").title("Planning").cal("a").build(),
        ev("same-id").title("Review").cal("b").build(),
    ];
    check_equivalent(&events, &LessonBook::default());
}
