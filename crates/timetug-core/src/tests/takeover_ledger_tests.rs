//! Port of `TakeoverLedgerTests.swift`.

use calendar_core::time::add_seconds;
use jiff::Timestamp;

use super::support::{date, make_event, opted_in, recorded_at};
use super::takeover_test_support::{ev, ev_at, ev_titled, merged_member};
use crate::model::event::TimeTugCalendarEvent;
use crate::takeover::ledger::{Entry, TakeoverLedger};
use crate::takeover::scheduler::Scheduler;

fn round_trip(ledger: &TakeoverLedger) -> TakeoverLedger {
    serde_json::from_str(&serde_json::to_string(ledger).unwrap()).unwrap()
}

/// `makeEvent(id, title: title, start: start, minutes: minutes)`.
fn event(id: &str, title: &str, start: &str, minutes: i64) -> TimeTugCalendarEvent {
    use super::support::{EventSpec, make_event_with};
    make_event_with(EventSpec {
        id: id.to_string(),
        title: title.to_string(),
        start: start.to_string(),
        minutes,
        ..EventSpec::default()
    })
}

fn far_future() -> Timestamp {
    Timestamp::from_second(64_000_000_000).unwrap()
}

#[test]
fn content_key_ignores_source_identity_and_case() {
    let a = ev_titled("a", "Standup");
    let b = ev_titled("b", "STANDUP");
    assert_ne!(a.id(), b.id());
    assert_eq!(a.content_key(), b.content_key());
    assert_ne!(
        a.content_key(),
        ev_at("a", "2026-09-18T11:00:00Z").content_key()
    );
}

#[test]
fn ledger_codable_round_trip_preserves_fired_and_snoozed() {
    let fired = ev_titled("f", "Fired");
    let snoozed = event("s", "Snoozed", "2026-09-18T10:00:00Z", 60);
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&fired, recorded_at());
    ledger.snooze(&snoozed, 300.0, date("2026-09-18T09:59:00Z"));
    let loaded = round_trip(&ledger);
    assert_eq!(loaded, ledger);
    assert!(loaded.has_fired(&fired));
    assert!(!loaded.has_fired(&snoozed));
    assert_eq!(
        loaded.entry_for(&snoozed),
        Some(Entry::Snoozed {
            until: date("2026-09-18T10:04:00Z")
        })
    );
}

#[test]
fn ledger_pruning_survives_round_trip() {
    let done = event("done", "Done", "2026-09-18T08:00:00Z", 30);
    let running = ev_titled("running", "Running");
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&done, recorded_at());
    ledger.mark_fired(&running, recorded_at());
    let mut loaded = round_trip(&ledger);
    loaded.prune(date("2026-09-18T10:10:00Z"));
    assert_eq!(loaded.entry_for(&done), None);
    assert_eq!(loaded.entry_for(&running), Some(Entry::Fired));
}

#[test]
fn prune_removes_both_keys() {
    let event = ev("a");
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&event, recorded_at());
    ledger.prune(date("2026-09-18T10:31:00Z"));
    // Same content, other id: the content key is gone too.
    assert_eq!(ledger.entry_for(&ev("other")), None);
    assert_eq!(ledger, TakeoverLedger::new());
}

#[test]
fn different_source_id_with_same_content_counts_as_fired() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&ev("old-id"), recorded_at());
    let resynced = ev("new-id");
    assert!(ledger.has_fired(&resynced));
    assert!(
        Scheduler::next(
            &[resynced],
            &opted_in(|_| {}),
            &ledger,
            date("2026-09-18T09:59:30Z")
        )
        .is_none()
    );
}

#[test]
fn different_content_and_id_is_not_the_same_meeting() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&ev("a"), recorded_at());
    // Same id: an edit, not a new meeting.
    assert!(ledger.has_fired(&ev_titled("a", "Renamed")));
    assert!(!ledger.has_fired(&ev_titled("b", "Renamed")));
    assert!(!ledger.has_fired(&ev_at("a", "2026-09-18T11:00:00Z")));
}

#[test]
fn has_fired_is_false_for_snoozed_and_unknown() {
    let event = make_event();
    let mut ledger = TakeoverLedger::new();
    assert!(!ledger.has_fired(&event));
    ledger.snooze(&event, 60.0, date("2026-09-18T10:00:00Z"));
    assert!(!ledger.has_fired(&event));
    assert!(
        Scheduler::next(
            &[event],
            &opted_in(|_| {}),
            &ledger,
            date("2026-09-18T10:00:00Z")
        )
        .is_some()
    );
}

#[test]
fn snooze_is_found_through_content_key() {
    let mut ledger = TakeoverLedger::new();
    ledger.snooze(
        &event("old", "Standup", "2026-09-18T10:00:00Z", 60),
        300.0,
        date("2026-09-18T09:59:00Z"),
    );
    let next = Scheduler::next(
        &[event("new", "Standup", "2026-09-18T10:00:00Z", 60)],
        &opted_in(|_| {}),
        &ledger,
        date("2026-09-18T09:59:00Z"),
    )
    .unwrap();
    assert_eq!(next.fire_at, date("2026-09-18T10:04:00Z"));
}

#[test]
fn acknowledge_in_progress_marks_only_unledgered_running_events() {
    let now = date("2026-09-18T10:10:00Z");
    let running = event("running", "Running", "2026-09-18T10:00:00Z", 30);
    let just_started = event("recent", "Recent", "2026-09-18T10:09:00Z", 30);
    let future = event("future", "Future", "2026-09-18T11:00:00Z", 30);
    let ended = event("ended", "Ended", "2026-09-18T09:00:00Z", 30);
    let snoozed = event("snoozed", "Snoozed", "2026-09-18T09:55:00Z", 60);
    let mut ledger = TakeoverLedger::new();
    ledger.snooze(&snoozed, 300.0, date("2026-09-18T10:08:00Z"));
    let count = ledger.acknowledge_in_progress(
        &[
            running.clone(),
            just_started.clone(),
            future.clone(),
            ended.clone(),
            snoozed.clone(),
        ],
        now,
        120.0,
    );
    assert_eq!(count, 1);
    assert!(ledger.has_fired(&running));
    assert_eq!(ledger.entry_for(&just_started), None);
    assert_eq!(ledger.entry_for(&future), None);
    assert_eq!(ledger.entry_for(&ended), None);
    assert!(!ledger.has_fired(&snoozed));
}

#[test]
fn is_snoozed_and_key_count_reflect_ledger_state() {
    let event = make_event();
    let mut ledger = TakeoverLedger::new();
    assert_eq!(ledger.key_count(), 0);
    assert!(!ledger.is_snoozed(&event));
    ledger.snooze(&event, 60.0, date("2026-09-18T10:00:00Z"));
    assert!(ledger.is_snoozed(&event));
    assert_eq!(ledger.key_count(), 2);
    ledger.mark_fired(&event, recorded_at());
    assert!(!ledger.is_snoozed(&event));
}

// MARK: Retention

#[test]
fn prune_reports_whether_anything_changed() {
    let event = make_event();
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&event, recorded_at());
    let changed1 = ledger.prune(date("2026-09-18T10:10:00Z"));
    assert!(!changed1);
    let changed2 = ledger.prune(date("2026-09-18T10:31:00Z"));
    assert!(changed2);
    let changed3 = ledger.prune(date("2026-09-18T10:32:00Z"));
    assert!(!changed3);
}

#[test]
fn far_future_event_is_kept_until_retention_elapses() {
    // A bogus year-4001 end would never be pruned by the end rule.
    let mut event = make_event();
    event.set_end(far_future());
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&event, recorded_at());
    let changed4 = ledger.prune(add_seconds(recorded_at(), TakeoverLedger::RETENTION - 1.0));
    assert!(!changed4);
    assert!(ledger.has_fired(&event));
    let changed5 = ledger.prune(add_seconds(recorded_at(), TakeoverLedger::RETENTION + 1.0));
    assert!(changed5);
    assert_eq!(ledger, TakeoverLedger::new());
}

#[test]
fn retention_is_seven_days() {
    assert_eq!(TakeoverLedger::RETENTION, 7.0 * 24.0 * 60.0 * 60.0);
    assert_eq!(TakeoverLedger::MAX_ENTRIES, 2000);
}

#[test]
fn size_cap_drops_oldest_by_recorded_at() {
    let mut ledger = TakeoverLedger::new();
    let total = TakeoverLedger::MAX_ENTRIES + 3;
    for i in 0..total {
        let mut event = ev_titled(&format!("e{i}"), &format!("T{i}"));
        event.set_end(far_future());
        ledger.mark_fired(&event, add_seconds(recorded_at(), i as f64));
    }
    let changed6 = ledger.prune(add_seconds(recorded_at(), total as f64));
    assert!(changed6);
    assert_eq!(ledger.key_count(), TakeoverLedger::MAX_ENTRIES * 2);
    let mut oldest = ev_titled("e0", "T0");
    oldest.set_end(far_future());
    let mut newest = ev_titled(&format!("e{}", total - 1), &format!("T{}", total - 1));
    newest.set_end(oldest.end());
    assert!(!ledger.has_fired(&oldest));
    assert!(ledger.has_fired(&newest));
}

#[test]
fn decoding_entry_without_recorded_at_does_not_crash() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&make_event(), recorded_at());
    let mut json = serde_json::to_string(&ledger).unwrap();
    // Strip every recordedAt member, simulating a file from before the field existed.
    json = json.replace("\"recordedAt\"", "\"ignored\"");
    let loaded: TakeoverLedger = serde_json::from_str(&json).unwrap();
    // Dropped rather than trusted.
    assert!(!loaded.has_fired(&make_event()));
}

fn merged(
    primary: &TimeTugCalendarEvent,
    with_others: &[TimeTugCalendarEvent],
) -> TimeTugCalendarEvent {
    let mut event = primary.clone();
    let mut all = vec![primary.clone()];
    all.extend(with_others.iter().cloned());
    event.merged_members = all.iter().map(merged_member).collect();
    event
}

#[test]
fn merged_event_counts_as_fired_when_any_member_fired() {
    let original = ev_titled("1", "Scott: Doctor");
    let official = ev_titled("2", "Intermountain Health");
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&original, recorded_at());
    assert!(ledger.has_fired(&merged(&official, &[original])));
}

#[test]
fn firing_merged_event_marks_every_member() {
    let original = ev_titled("1", "Scott: Doctor");
    let official = ev_titled("2", "Intermountain Health");
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(
        &merged(&official, std::slice::from_ref(&original)),
        recorded_at(),
    );
    assert!(ledger.has_fired(&original));
    assert!(ledger.has_fired(&official));
}

// Not in the Swift file: the rule that a fired entry wins over a snoozed one found through another key (the
// `entry(for:)` `first(where: == .fired) ?? first` line), which `takeover/ledger.json` also pins.
#[test]
fn a_fired_entry_wins_over_a_snoozed_one_found_through_another_key() {
    let a = ev_titled("a", "Standup");
    let copy = ev_titled("b", "STANDUP");
    let mut ledger = TakeoverLedger::new();
    ledger.snooze(&a, 300.0, recorded_at());
    ledger.mark_fired(&copy, add_seconds(recorded_at(), 60.0));
    assert!(ledger.has_fired(&a));
    assert!(!ledger.is_snoozed(&a));
}
