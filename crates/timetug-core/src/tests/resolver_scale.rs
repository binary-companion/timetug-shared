//! Review Focus 2 of the phase 2 plan: forty near-duplicate events at once, a shared calendar seen through four
//! accounts, must give the same groups as Swift and stay fast.
//!
//! "The same groups as Swift" is checked two ways: the resolver output equals the ported frozen baseline resolver
//! (`BaselineDuplicateResolver`, which the Swift equivalence tests use as the oracle), and
//! `vectors/dedup/resolutions.json` pins the Swift output for the same kind of input (its generated cases hold up to 32
//! events). The titles of one meeting differ only by case, accents and spacing: they all fold to one normal form.

use std::collections::BTreeSet;
use std::time::Instant;

use super::baseline_duplicate_resolver::BaselineDuplicateResolver;
use super::dedup_support::ev;
use crate::dedup::adjudication::VerdictCache;
use crate::dedup::lesson_book::LessonBook;
use crate::dedup::resolver::DuplicateResolver;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::merge_types::MergeProvenance;

const MEETINGS: usize = 10;
const ACCOUNTS: usize = 4;

/// Ten meetings, each seen through four accounts (`s1` to `s4`, one calendar `shared` per account), titled with the
/// four spellings that Swift folds together.
fn forty_near_duplicates() -> Vec<TimeTugCalendarEvent> {
    let mut events = Vec::new();
    for meeting in 0..MEETINGS {
        let spellings = [
            format!("Réunion équipe {meeting}"),
            format!("REUNION EQUIPE {meeting}"),
            format!("réunion   équipe {meeting}"),
            format!("Reunion, equipe {meeting}!"),
        ];
        // Meetings start every ten minutes inside one hour, so their time gates overlap a lot.
        let start = format!("2026-09-18T10:{:02}:00Z", meeting * 5);
        for (account, title) in spellings.iter().enumerate() {
            let mut event = ev(&format!("m{meeting}-a{account}"))
                .title(title)
                .start(&start)
                .minutes(30)
                .cal("shared")
                .build();
            event.source_id = format!("s{}", account + 1);
            events.push(event);
        }
    }
    assert_eq!(events.len(), MEETINGS * ACCOUNTS);
    events
}

#[test]
fn forty_near_duplicates_resolve_to_ten_groups_and_match_the_baseline() {
    let events = forty_near_duplicates();
    for inference_on in [false, true] {
        let verdicts = inference_on.then(VerdictCache::default);
        let actual =
            DuplicateResolver::resolve(&events, &[], &LessonBook::default(), verdicts.as_ref());
        let baseline = BaselineDuplicateResolver::resolve(
            &events,
            &[],
            &LessonBook::default(),
            verdicts.as_ref(),
        );
        assert_eq!(actual.events, baseline.events);
        assert_eq!(actual.pending, baseline.pending);
        assert_eq!(actual.candidates, baseline.candidates);

        assert_eq!(actual.events.len(), MEETINGS);
        let groups: BTreeSet<BTreeSet<String>> = actual
            .events
            .iter()
            .map(|e| {
                e.merged_members
                    .iter()
                    .map(|m| m.calendar_key.clone())
                    .collect()
            })
            .collect();
        let every_account: BTreeSet<String> =
            (1..=ACCOUNTS).map(|a| format!("s{a}/shared")).collect();
        assert_eq!(groups, BTreeSet::from([every_account]));
        for event in &actual.events {
            assert_eq!(event.merged_members.len(), ACCOUNTS);
            assert_eq!(event.merge_provenance, Some(MergeProvenance::Rule));
        }
    }
}

#[test]
#[allow(clippy::print_stderr)]
fn forty_near_duplicates_resolve_fast() {
    let events = forty_near_duplicates();
    let book = LessonBook::default();
    let verdicts = VerdictCache::default();
    let started = Instant::now();
    const RUNS: u32 = 20;
    for _ in 0..RUNS {
        let result = DuplicateResolver::resolve(&events, &[], &book, Some(&verdicts));
        assert_eq!(result.events.len(), MEETINGS);
    }
    let per_run = started.elapsed() / RUNS;
    eprintln!("resolve(40 near-duplicates, inference on): {per_run:?} per run (test profile)");
    // The Swift baseline resolves 32 events in well under a millisecond in release; even an unoptimized test build
    // must stay far below a frame.
    assert!(per_run.as_millis() < 250, "{per_run:?}");
}
