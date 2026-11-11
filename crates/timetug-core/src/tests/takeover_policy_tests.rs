//! Port of `TakeoverPolicyTests.swift`.

use calendar_core::model::ResponseStatus;

use super::support::{EventSpec, make_event, make_event_with, opted_in};
use super::takeover_test_support::{ev_on, ev_others, ev_status, ev_with_link};
use crate::takeover::policy::TakeoverPolicy;

#[test]
fn qualifies_when_opted_in_and_defaults_pass() {
    assert!(TakeoverPolicy::qualifies(&make_event(), &opted_in(|_| {})));
}

#[test]
fn rejects_calendar_not_opted_in() {
    assert!(!TakeoverPolicy::qualifies(
        &ev_on("family"),
        &opted_in(|_| {})
    ));
}

#[test]
fn rejects_all_day_by_default() {
    let all_day = make_event_with(EventSpec {
        is_all_day: true,
        ..EventSpec::default()
    });
    assert!(!TakeoverPolicy::qualifies(&all_day, &opted_in(|_| {})));
}

#[test]
fn all_day_never_takes_over_even_when_skip_all_day_is_off() {
    let all_day = make_event_with(EventSpec {
        is_all_day: true,
        ..EventSpec::default()
    });
    assert!(!TakeoverPolicy::qualifies(
        &all_day,
        &opted_in(|s| s.skip_all_day_events = false)
    ));
}

#[test]
fn always_rejects_declined_events() {
    let declined = ev_status(ResponseStatus::Declined);
    assert!(!TakeoverPolicy::qualifies(&declined, &opted_in(|_| {})));
    assert!(!TakeoverPolicy::qualifies(
        &declined,
        &opted_in(|s| {
            s.require_conference_link = true;
            s.require_other_attendees = true;
        })
    ));
}

#[test]
fn solo_events_qualify_by_default_and_are_rejected_when_other_attendees_required() {
    let solo = ev_others(0);
    assert!(TakeoverPolicy::qualifies(&solo, &opted_in(|_| {})));
    assert!(!TakeoverPolicy::qualifies(
        &solo,
        &opted_in(|s| s.require_other_attendees = true)
    ));
    assert!(TakeoverPolicy::qualifies(
        &ev_others(1),
        &opted_in(|s| s.require_other_attendees = true)
    ));
}

#[test]
fn require_conference_link_filters_events_without_one() {
    let settings = opted_in(|s| s.require_conference_link = true);
    assert!(!TakeoverPolicy::qualifies(&make_event(), &settings));
    let with_link = ev_with_link("https://meet.google.com/a-b-c");
    assert!(TakeoverPolicy::qualifies(&with_link, &settings));
}

#[test]
fn a_hidden_calendar_never_tugs_even_if_opted_in() {
    let hidden = opted_in(|s| {
        s.hidden_calendar_keys = ["fake/cal".to_string()].into();
    });
    assert!(!TakeoverPolicy::qualifies(&make_event(), &hidden));
}

#[test]
fn a_merged_event_needs_an_opted_in_copy_that_is_shown() {
    let mut event = ev_on("family");
    event.additional_calendar_keys = ["fake/cal".to_string()].into();
    assert!(TakeoverPolicy::qualifies(&event, &opted_in(|_| {})));
    assert!(!TakeoverPolicy::qualifies(
        &event,
        &opted_in(|s| s.hidden_calendar_keys = ["fake/cal".to_string()].into())
    ));
}

#[test]
fn qualifies_when_duplicate_on_an_opted_in_calendar() {
    let mut event = ev_on("family");
    event.additional_calendar_keys = ["fake/cal".to_string()].into();
    assert!(TakeoverPolicy::qualifies(&event, &opted_in(|_| {})));
    event.additional_calendar_keys = Default::default();
    assert!(!TakeoverPolicy::qualifies(&event, &opted_in(|_| {})));
}

#[test]
fn turning_tug_off_blocks_an_otherwise_qualifying_event() {
    assert!(TakeoverPolicy::qualifies(&make_event(), &opted_in(|_| {})));
    assert!(!TakeoverPolicy::qualifies(
        &make_event(),
        &opted_in(|s| s.enabled = false)
    ));
}
