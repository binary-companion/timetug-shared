//! Port of `TimeTugCoreTests/DuplicateRulesTests.swift`.

use calendar_core::model::{Attendee, UidScope};

use super::dedup_support::{attendee, ev, url};
use crate::dedup::rules::{DuplicateRules, MergeReason, PairDecision, SeparateReason};
use crate::model::event::TimeTugCalendarEvent;

fn decide(a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) -> PairDecision {
    DuplicateRules::decide(a, b)
}

const MERGE_EXACT: PairDecision = PairDecision::Merge(MergeReason::ExactMatch);

#[test]
fn exact_match_merges_even_on_one_calendar() {
    assert_eq!(
        decide(
            &ev("1").title("Sync").build(),
            &ev("2").title("sync").cal("other").build()
        ),
        MERGE_EXACT
    );
    assert_eq!(
        decide(
            &ev("1").title("Sync").build(),
            &ev("2").title("Sync").build()
        ),
        MERGE_EXACT
    );
}

#[test]
fn same_calendar_different_titles_stay_separate() {
    assert_eq!(
        decide(
            &ev("1").title("Doctor").build(),
            &ev("2").title("Dance").build()
        ),
        PairDecision::Separate(SeparateReason::SameCalendar)
    );
}

#[test]
fn all_day_events_only_merge_on_exact_match() {
    let a = ev("1").title("Birthday").all_day().build();
    let b = ev("2")
        .title("Kristin birthday")
        .cal("other")
        .all_day()
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Separate(SeparateReason::AllDay)
    );
}

#[test]
fn time_gate_boundaries() {
    let a = ev("1").title("A").minutes(60).build(); // 10:00-11:00
    let b = |start: &str, minutes: i64| {
        ev("2")
            .title("B")
            .start(start)
            .minutes(minutes)
            .cal("o")
            .build()
    };
    assert_eq!(
        decide(&a, &b("2026-09-18T10:30:00Z", 60)),
        PairDecision::Ambiguous
    );
    assert_eq!(
        decide(&a, &b("2026-09-18T10:31:00Z", 60)),
        PairDecision::Separate(SeparateReason::OutsideTimeGate)
    );
    // The end differs by 60 minutes: inside; by 61: outside.
    assert_eq!(
        decide(
            &ev("1").title("A").minutes(30).build(),
            &ev("2").title("B").minutes(90).cal("o").build()
        ),
        PairDecision::Ambiguous
    );
    assert_eq!(
        decide(
            &ev("1").title("A").minutes(30).build(),
            &ev("2").title("B").minutes(91).cal("o").build()
        ),
        PairDecision::Separate(SeparateReason::OutsideTimeGate)
    );
}

#[test]
fn shared_conference_merges_despite_different_titles_and_queries() {
    let a = ev("1")
        .title("Weekly")
        .location("https://acme.zoom.us/j/555?pwd=abc")
        .build();
    let b = ev("2")
        .title("Team sync")
        .cal("o")
        .notes("Join https://acme.zoom.us/j/555/")
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::ConferenceLink)
    );
}

#[test]
fn shared_external_uid_merges() {
    let a = ev("1").title("A").uid("uid-1").build();
    let b = ev("2").title("B").cal("o").uid("uid-1").build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::ExternalUid)
    );
}

#[test]
fn shared_attendee_email_merges_case_insensitively() {
    let a = ev("1")
        .title("A")
        .attendees(vec![attendee("Kristin@x.com")])
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .attendees(vec![attendee("kristin@X.com"), attendee("z@x.com")])
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::SharedAttendee)
    );
}

#[test]
fn shared_attendee_does_not_override_a_conflicting_location() {
    let a = ev("1")
        .title("A")
        .location("Room 101 East")
        .attendees(vec![attendee("k@x.com")])
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .location("Dentist Office")
        .attendees(vec![attendee("k@x.com")])
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Separate(SeparateReason::ConflictingLocation)
    );
}

#[test]
fn shared_attendee_does_not_override_different_conference_links() {
    let a = ev("1")
        .title("A")
        .conf("https://acme.zoom.us/j/1")
        .attendees(vec![attendee("k@x.com")])
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .conf("https://acme.zoom.us/j/2")
        .attendees(vec![attendee("k@x.com")])
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Separate(SeparateReason::ConflictingConference)
    );
}

fn organizer(email: &str) -> Attendee {
    Attendee::new(
        None,
        Some(email),
        calendar_core::model::AttendeeRole::Required,
        calendar_core::model::ResponseStatus::NeedsAction,
        false,
        true,
    )
}

#[test]
fn shared_organizer_alone_merges_when_nothing_conflicts() {
    let mut a = ev("1").title("A").build();
    a.event.organizer = Some(organizer("Boss@x.com"));
    let mut b = ev("2").title("B").cal("o").build();
    b.event.organizer = Some(organizer("boss@x.com"));
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::SharedAttendee)
    );
}

#[test]
fn shared_attendee_with_same_location_reports_shared_attendee() {
    // Shared attendee is checked before same location, so it supplies the reason.
    let a = ev("1")
        .title("A")
        .location("Room 101 East")
        .attendees(vec![attendee("k@x.com")])
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .location("Room 101 East")
        .attendees(vec![attendee("k@x.com")])
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::SharedAttendee)
    );
}

#[test]
fn same_location_does_not_override_different_conference_links() {
    let a = ev("1")
        .title("A")
        .location("Room 101 East")
        .conf("https://acme.zoom.us/j/1")
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .location("Room 101 East")
        .conf("https://acme.zoom.us/j/2")
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Separate(SeparateReason::ConflictingConference)
    );
}

#[test]
fn same_location_merges_including_containment() {
    let a = ev("1").title("Doctor").location("1234 Main St").build();
    let b = ev("2")
        .title("Intermountain Health")
        .cal("o")
        .location("Intermountain Health, 1234 Main St")
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::SameLocation)
    );
}

#[test]
fn vetoes_fire_only_when_both_sides_have_the_field() {
    let loc1 = ev("1").title("A").location("Room 101 East").build();
    let loc2 = ev("2")
        .title("B")
        .cal("o")
        .location("Dentist Office")
        .build();
    assert_eq!(
        decide(&loc1, &loc2),
        PairDecision::Separate(SeparateReason::ConflictingLocation)
    );

    let z1 = ev("1").title("A").conf("https://acme.zoom.us/j/1").build();
    let z2 = ev("2")
        .title("B")
        .cal("o")
        .conf("https://acme.zoom.us/j/2")
        .build();
    assert_eq!(
        decide(&z1, &z2),
        PairDecision::Separate(SeparateReason::ConflictingConference)
    );

    let p1 = ev("1")
        .title("A")
        .attendees(vec![attendee("a@x.com")])
        .build();
    let p2 = ev("2")
        .title("B")
        .cal("o")
        .attendees(vec![attendee("b@x.com")])
        .build();
    assert_eq!(
        decide(&p1, &p2),
        PairDecision::Separate(SeparateReason::ConflictingAttendees)
    );

    // Absent on one side is not a veto.
    assert_eq!(
        decide(&loc1, &ev("2").title("B").cal("o").build()),
        PairDecision::Ambiguous
    );
    assert_eq!(
        decide(
            &ev("1").title("Scott: Doctor").build(),
            &ev("2").title("Intermountain Health").cal("o").build()
        ),
        PairDecision::Ambiguous
    );
}

#[test]
fn detail_score_and_summary() {
    assert_eq!(
        DuplicateRules::detail_summary(&ev("e1").others(0).build()),
        "bare"
    );
    let rich = ev("e1")
        .others(2)
        .location("1 Main St")
        .notes("Bring card")
        .build();
    assert_eq!(
        DuplicateRules::detail_summary(&rich),
        "location+attendees+notes"
    );
    assert_eq!(DuplicateRules::detail_score(&rich), 3);
}

#[test]
fn participants_are_the_event_itself_when_never_merged() {
    let event = ev("1").title("Doctor").build();
    let titles: Vec<String> = event.participants().into_iter().map(|m| m.title).collect();
    assert_eq!(titles, ["Doctor"]);
    assert_eq!(event.participants()[0].calendar_key, "fake/cal");
}

#[test]
fn webex_links_are_identified_by_their_meeting_id_in_the_query() {
    let a = ev("1")
        .title("Weekly")
        .cal("a")
        .conf("https://acme.webex.com/acme/j.php?MTID=m111")
        .build();
    let same_meeting = ev("2")
        .title("Team sync")
        .cal("b")
        .conf("https://acme.webex.com/acme/j.php?MTID=M111&x=1")
        .build();
    let other = ev("3")
        .title("Team sync")
        .cal("b")
        .conf("https://acme.webex.com/acme/j.php?MTID=m222")
        .build();
    assert_eq!(
        decide(&a, &same_meeting),
        PairDecision::Merge(MergeReason::ConferenceLink)
    );
    assert_ne!(
        decide(&a, &other),
        PairDecision::Merge(MergeReason::ConferenceLink)
    );
}

#[test]
fn a_plain_website_in_the_url_field_is_not_a_conference_identity() {
    let page = "https://example.com/event/42";
    let a = ev("1").title("Weekly").cal("a").url(page).build();
    let b = ev("2").title("Team sync").cal("b").url(page).build();
    assert!(DuplicateRules::conference_identities(&a).is_empty());
    assert_ne!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::ConferenceLink)
    );
}

#[test]
fn unlisted_providers_still_veto_when_the_source_supplies_the_link() {
    let mail = || vec![attendee("k@x.com")];
    let a = ev("1")
        .title("A")
        .conf("https://chime.aws/111")
        .attendees(mail())
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .conf("https://chime.aws/222")
        .attendees(mail())
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Separate(SeparateReason::ConflictingConference)
    );
    let c = ev("3")
        .title("C")
        .cal("o")
        .conf("https://chime.aws/111")
        .attendees(mail())
        .build();
    assert_eq!(
        decide(&a, &c),
        PairDecision::Merge(MergeReason::ConferenceLink)
    );
}

#[test]
fn a_detected_generic_url_is_not_a_conference_identity() {
    let a = ev("1").title("A").url("https://example.com/x").build();
    assert!(DuplicateRules::conference_identities(&a).is_empty());
}

#[test]
fn same_title_inside_the_time_gate_merges_even_when_the_ends_differ() {
    let a = ev("1")
        .title("Dance Party")
        .start("2026-09-18T19:15:00Z")
        .minutes(45)
        .cal("shared")
        .build();
    let b = ev("2")
        .title("Dance Party")
        .start("2026-09-18T19:15:00Z")
        .minutes(60)
        .cal("blackout")
        .build();
    assert_eq!(decide(&a, &b), PairDecision::Merge(MergeReason::SameTitle));
    let later = ev("3")
        .title("dance  party!")
        .start("2026-09-18T19:30:00Z")
        .minutes(45)
        .cal("blackout")
        .build();
    // Punctuation, case and spacing do not matter; a 15 min start gap is inside the gate.
    assert_eq!(
        decide(&a, &later),
        PairDecision::Merge(MergeReason::SameTitle)
    );
}

#[test]
fn same_title_still_needs_the_time_gate_and_a_different_calendar() {
    let a = ev("1").title("Dance Party").minutes(60).build();
    assert_eq!(
        decide(
            &a,
            &ev("2")
                .title("Dance Party")
                .start("2026-09-18T10:31:00Z")
                .minutes(60)
                .cal("o")
                .build()
        ),
        PairDecision::Separate(SeparateReason::OutsideTimeGate)
    );
    assert_eq!(
        decide(&a, &ev("3").title("Dance Party").minutes(90).build()),
        PairDecision::Separate(SeparateReason::SameCalendar)
    );
}

#[test]
fn same_title_does_not_override_a_conflicting_place_or_people() {
    let a = ev("1")
        .title("Standup")
        .location("Room A")
        .attendees(vec![attendee("x@acme.com")])
        .build();
    let other_room = ev("2")
        .title("Standup")
        .minutes(45)
        .cal("o")
        .location("Room B")
        .build();
    assert_eq!(
        decide(&a, &other_room),
        PairDecision::Separate(SeparateReason::ConflictingLocation)
    );
    let other_people = ev("3")
        .title("Standup")
        .minutes(45)
        .cal("o")
        .attendees(vec![attendee("y@acme.com")])
        .build();
    assert_eq!(
        decide(&a, &other_people),
        PairDecision::Separate(SeparateReason::ConflictingAttendees)
    );
}

// UID scope (Issue 9).

#[test]
fn uid_match_key_depends_on_the_scope() {
    assert_eq!(
        ev("1")
            .uid("u")
            .scope(UidScope::Global)
            .build()
            .uid_match_key(),
        Some("u".to_string())
    );
    assert_eq!(
        ev("1")
            .uid("u")
            .scope(UidScope::Provider)
            .service("eventkit")
            .provider("microsoft")
            .build()
            .uid_match_key(),
        Some("eventkit|microsoft|u".to_string())
    );
    assert_eq!(
        ev("1").uid("u").service("eventkit").build().uid_match_key(),
        Some("eventkit|?|u".to_string())
    );
    assert_eq!(
        ev("1").uid("u").build().uid_match_key(),
        Some("?|?|u".to_string())
    );
    assert_eq!(ev("1").build().uid_match_key(), None);
}

#[test]
fn exchange_copies_inside_one_store_still_merge_on_their_id() {
    let exchange = |id: &str, title: &str, cal: &str| {
        ev(id)
            .title(title)
            .cal(cal)
            .uid("exch-1")
            .scope(UidScope::Provider)
            .service("eventkit")
            .provider("microsoft")
            .build()
    };
    assert_eq!(
        decide(&exchange("1", "A", "cal"), &exchange("2", "B", "o")),
        PairDecision::Merge(MergeReason::ExternalUid)
    );
}

#[test]
fn an_exchange_id_never_matches_another_services_uid() {
    let exchange = ev("1")
        .title("A")
        .uid("same")
        .scope(UidScope::Provider)
        .service("eventkit")
        .provider("microsoft")
        .build();
    let google = ev("2")
        .title("B")
        .cal("o")
        .uid("same")
        .scope(UidScope::Global)
        .service("google")
        .provider("google")
        .build();
    assert_ne!(
        decide(&exchange, &google),
        PairDecision::Merge(MergeReason::ExternalUid)
    );
}

#[test]
fn global_uids_match_across_services() {
    let a = ev("1")
        .title("A")
        .uid("same")
        .scope(UidScope::Global)
        .service("eventkit")
        .provider("icloud")
        .build();
    let b = ev("2")
        .title("B")
        .cal("o")
        .uid("same")
        .scope(UidScope::Global)
        .service("google")
        .provider("google")
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::ExternalUid)
    );
}

#[test]
fn copies_whose_attendee_is_an_email_on_each_side_merge_on_the_shared_attendee() {
    // One copy's attendee came from a mailto address, the other's was resolved from Contacts: both are just emails.
    let a = ev("1")
        .title("Review")
        .attendees(vec![attendee("bo@x.test")])
        .build();
    let b = ev("2")
        .title("Design review")
        .cal("o")
        .attendees(vec![attendee("Bo@X.test")])
        .build();
    assert_eq!(
        decide(&a, &b),
        PairDecision::Merge(MergeReason::SharedAttendee)
    );
}

// The URL helper is exercised through `ev(..).url(..)`; keep the import used for clarity of the Swift `URL(string:)`.
#[test]
fn url_helper_keeps_original_text() {
    assert_eq!(
        url("https://acme.zoom.us/j/1").as_str(),
        "https://acme.zoom.us/j/1"
    );
}
