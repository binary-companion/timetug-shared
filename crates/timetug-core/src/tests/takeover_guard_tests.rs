//! Port of `TakeoverGuardTests.swift`.

use calendar_core::model::ResponseStatus;
use jiff::Timestamp;

use super::support::{date, make_event, opted_in, recorded_at};
use super::takeover_test_support::{ev, ev_titled, merged_member};
use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use crate::takeover::guard::{Decision, Reason, TakeoverGuard};
use crate::takeover::ledger::TakeoverLedger;

fn now() -> Timestamp {
    date("2026-09-18T09:59:00Z")
}

/// The Swift file's private `evaluate(_:current:settings:ledger:now:overlayVisible:)` at its default arguments.
struct Args {
    current: Option<Vec<TimeTugCalendarEvent>>,
    settings: TakeoverSettings,
    ledger: TakeoverLedger,
    now: Timestamp,
    overlay_visible: bool,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            current: None,
            settings: opted_in(|_| {}),
            ledger: TakeoverLedger::new(),
            now: now(),
            overlay_visible: false,
        }
    }
}

fn evaluate(event: &TimeTugCalendarEvent, args: Args) -> Decision {
    let current = args.current.unwrap_or_else(|| vec![event.clone()]);
    TakeoverGuard::evaluate(
        event,
        &current,
        &args.settings,
        &args.ledger,
        args.now,
        args.overlay_visible,
    )
}

#[test]
fn guard_presents_normal_event() {
    assert_eq!(evaluate(&make_event(), Args::default()), Decision::Present);
}

#[test]
fn guard_suppresses_when_overlay_visible_first() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&make_event(), recorded_at());
    assert_eq!(
        evaluate(
            &make_event(),
            Args {
                current: Some(vec![]),
                ledger,
                overlay_visible: true,
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::OverlayVisible)
    );
}

#[test]
fn guard_suppresses_event_missing_from_snapshot() {
    assert_eq!(
        evaluate(
            &make_event(),
            Args {
                current: Some(vec![ev_titled("other", "Other")]),
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::NotInSnapshot)
    );
    assert_eq!(
        evaluate(
            &make_event(),
            Args {
                current: Some(vec![]),
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::NotInSnapshot)
    );
}

#[test]
fn guard_matches_snapshot_by_content_key() {
    assert_eq!(
        evaluate(
            &ev("old"),
            Args {
                current: Some(vec![ev("new")]),
                ..Args::default()
            }
        ),
        Decision::Present
    );
}

#[test]
fn guard_suppresses_ended_before_qualification() {
    let event = make_event();
    let late = date("2026-09-18T10:30:00Z");
    let mut declined = event.clone();
    declined.response_status = Some(ResponseStatus::Declined);
    assert_eq!(
        evaluate(
            &event,
            Args {
                now: late,
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::Ended)
    );
    assert_eq!(
        evaluate(
            &event,
            Args {
                current: Some(vec![declined]),
                now: late,
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::Ended)
    );
}

#[test]
fn guard_uses_edited_current_copy() {
    let event = make_event();
    let mut declined = event.clone();
    declined.response_status = Some(ResponseStatus::Declined);
    assert_eq!(
        evaluate(
            &event,
            Args {
                current: Some(vec![declined]),
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::NoLongerQualifies)
    );
    let mut opted_out = event.clone();
    opted_out.set_calendar_id("family");
    assert_eq!(
        evaluate(
            &event,
            Args {
                current: Some(vec![opted_out]),
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::NoLongerQualifies)
    );
}

#[test]
fn guard_suppresses_already_fired_and_checks_before_qualifying() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&make_event(), recorded_at());
    assert_eq!(
        evaluate(
            &make_event(),
            Args {
                ledger: ledger.clone(),
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::AlreadyFired)
    );
    let mut declined = make_event();
    declined.response_status = Some(ResponseStatus::Declined);
    assert_eq!(
        evaluate(
            &make_event(),
            Args {
                current: Some(vec![declined]),
                ledger,
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::NoLongerQualifies)
    );
}

#[test]
fn guard_suppresses_fired_through_content_key() {
    let mut ledger = TakeoverLedger::new();
    ledger.mark_fired(&ev("old"), recorded_at());
    assert_eq!(
        evaluate(
            &ev("new"),
            Args {
                ledger,
                ..Args::default()
            }
        ),
        Decision::Suppress(Reason::AlreadyFired)
    );
}

#[test]
fn guard_presents_snoozed_event() {
    let mut ledger = TakeoverLedger::new();
    ledger.snooze(&make_event(), 60.0, now());
    assert_eq!(
        evaluate(
            &make_event(),
            Args {
                ledger,
                ..Args::default()
            }
        ),
        Decision::Present
    );
}

#[test]
fn armed_copy_still_matches_after_it_was_merged_into_another_event() {
    let armed = ev_titled("1", "Scott: Doctor");
    let mut current = ev_titled("2", "Intermountain Health");
    current.merged_members = [armed.clone(), current.clone()]
        .iter()
        .map(merged_member)
        .collect();
    let decision = TakeoverGuard::evaluate(
        &armed,
        &[current],
        &opted_in(|_| {}),
        &TakeoverLedger::new(),
        date("2026-09-18T09:59:00Z"),
        false,
    );
    assert_ne!(decision, Decision::Suppress(Reason::NotInSnapshot));
}

#[test]
fn guard_suppresses_while_tug_is_off() {
    let event = make_event();
    let decision = TakeoverGuard::evaluate(
        &event,
        std::slice::from_ref(&event),
        &opted_in(|s| s.enabled = false),
        &TakeoverLedger::new(),
        date("2026-09-18T09:59:30Z"),
        false,
    );
    assert_eq!(decision, Decision::Suppress(Reason::NoLongerQualifies));
}
