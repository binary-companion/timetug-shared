//! The exported pure functions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use calendar_core::model::CalendarEvent as EngineEvent;
use calendar_core::time::Zone;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;

use crate::errors::FfiError;
use crate::functions::*;
use crate::values::model::{
    CalendarDate, CalendarEvent, ConferenceOrigin, ConferenceProvider, SourceCapabilities, SyncKind,
};
use crate::values::write::{EventField, EventPatch, FieldUpdateString};
use crate::values::{ToEngine, ToMirror};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

fn url(text: &str) -> UrlString {
    UrlString::parse(text).unwrap()
}

fn event() -> CalendarEvent {
    EngineEvent::new("e", "c", "Title", at(1_700_000_000), at(1_700_003_600)).to_mirror()
}

#[test]
fn the_join_policy_allows_web_links_and_zoom_only() {
    let cases = [
        ("https://meet.example.com/a", true),
        ("http://meet.example.com/a", true),
        ("zoommtg://us02web.zoom.us/join?confno=1", true),
        ("zoommtg://evil.example.com/join", false),
        ("file:///etc/passwd", false),
        ("https://user:pw@example.com/", false),
        ("javascript:alert(1)", false),
    ];
    for (text, allowed) in cases {
        assert_eq!(join_url_is_allowed(url(text)), allowed, "{text}");
    }
}

#[test]
fn a_calendar_user_address_becomes_an_email_or_nothing() {
    assert_eq!(
        calendar_user_address_email(Some("mailto:Ann@Example.com?subject=x".into())).as_deref(),
        Some("ann@example.com")
    );
    assert_eq!(
        calendar_user_address_email(Some("urn:uuid:1234".into())),
        None
    );
    assert_eq!(calendar_user_address_email(Some("  ".into())), None);
    assert_eq!(calendar_user_address_email(None), None);
}

#[test]
fn conferences_come_from_the_notes_and_the_location() {
    let found = detect_conferences(
        vec![],
        Some("Room 4".into()),
        None,
        Some("Join https://us02web.zoom.us/j/123456789 now".into()),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].provider, ConferenceProvider::Zoom);
    assert_eq!(found[0].origin, ConferenceOrigin::Notes);
    assert!(detect_conferences(vec![], None, None, Some("no link here".into())).is_empty());
}

#[test]
fn require_writable_names_the_fields_the_source_cannot_write() {
    let capabilities = SourceCapabilities {
        can_write: true,
        can_edit_attendees: false,
        can_respond_to_invite: false,
        provided_fields: HashSet::new(),
        sync_kind: SyncKind::None,
        supports_push: false,
        writable_fields: [EventField::Title].into(),
        controls_notifications: false,
        recurrence_scopes: HashSet::new(),
    };
    assert_eq!(
        require_writable([EventField::Title].into(), capabilities.clone()),
        Ok(())
    );
    let error = require_writable(
        [EventField::Title, EventField::Timing, EventField::Notes].into(),
        capabilities,
    )
    .unwrap_err();
    assert_eq!(
        error,
        FfiError::Write(crate::values::write::WriteError::Unsupported {
            fields: [EventField::Timing, EventField::Notes].into()
        })
    );
}

#[test]
fn a_patch_made_by_comparison_keeps_its_base_across_the_boundary() {
    let original = event();
    let mut edited = original.clone();
    edited.title = "Renamed".into();
    edited.notes = Some("new notes".into());
    let patch = event_patch_diff(original.clone(), edited);
    assert_eq!(patch.title.as_deref(), Some("Renamed"));
    assert_eq!(patch.notes, FieldUpdateString::Set("new notes".into()));
    assert_eq!(patch.base, Some(original.clone()));
    // Out to the host and back in: the base survives, so the conflict check still knows what the patch was made against.
    let back: EventPatch = patch.clone().to_engine().to_mirror();
    assert_eq!(back, patch);

    let mut changed_by_someone_else = original.clone();
    changed_by_someone_else.title = "Their title".into();
    assert_eq!(
        patch_merge_conflicts(patch.clone(), changed_by_someone_else),
        HashSet::from([EventField::Title])
    );
    assert!(patch_merge_conflicts(patch, original).is_empty());
}

#[test]
fn an_event_reference_is_built_from_the_event_as_read() {
    let mut read = event();
    read.version = Some("etag".into());
    let reference = event_ref_from_event(read);
    assert_eq!(
        (reference.calendar_id.as_str(), reference.event_id.as_str()),
        ("c", "e")
    );
    assert_eq!(reference.version.as_deref(), Some("etag"));
}

fn next_day() -> CalendarDate {
    CalendarDate {
        year: 2026,
        month: 11,
        day: 15,
    }
}

#[test]
fn all_day_helpers_agree_with_the_zone() {
    let zone = Zone::named("Pacific/Auckland").unwrap();
    let day = CalendarDate {
        year: 2026,
        month: 11,
        day: 14,
    };
    let start = all_day_start_of_day(day.clone(), zone.clone()).unwrap();
    assert_eq!(all_day_date_of(start, zone.clone()), day);
    let next = all_day_end_exclusive_after_last(day.clone());
    assert_eq!(
        next,
        CalendarDate {
            year: 2026,
            month: 11,
            day: 15
        }
    );
    let end = all_day_start_of_day(next, zone.clone()).unwrap();
    let dates = all_day_dates(start, end, zone);
    assert_eq!(dates.first, day);
    assert_eq!(dates.end_exclusive, next_day());
    assert_eq!(calendar_date_adding_days(dates.end_exclusive, -1), day);
    assert_eq!(
        calendar_date_adding_days(
            CalendarDate {
                year: 2026,
                month: 12,
                day: 31
            },
            1
        ),
        CalendarDate {
            year: 2027,
            month: 1,
            day: 1
        }
    );
    // The span's first day is the day it starts on; its end is the day after (one-day event: 14 to 15 November).
    // Foundation resolves a day that does not exist (31 February) forward, as the engine does.
    assert_eq!(
        all_day_date_of(
            all_day_start_of_day(
                CalendarDate {
                    year: 2026,
                    month: 2,
                    day: 31
                },
                Zone::utc()
            )
            .unwrap(),
            Zone::utc()
        ),
        CalendarDate {
            year: 2026,
            month: 3,
            day: 3
        }
    );
}

// ---------- the write checks (what the EventKit adapter calls before it touches the store) ----------

fn timing(start: i64, end: i64) -> crate::values::write::EventTiming {
    crate::values::write::EventTiming {
        start: at(start),
        end: at(end),
        time_zone: Some(Zone::utc()),
        is_all_day: false,
    }
}

fn draft() -> crate::values::write::EventDraft {
    calendar_core::write::event_draft::EventDraft::new(
        "Lunch",
        timing(1_700_000_000, 1_700_003_600).to_engine(),
    )
    .to_mirror()
}

#[test]
fn a_draft_uses_the_fields_it_sets_beyond_the_defaults() {
    let mut draft = draft();
    assert_eq!(
        event_draft_used_fields(draft.clone()),
        HashSet::from([EventField::Title, EventField::Timing])
    );
    draft.notes = Some("bring cake".into());
    draft.location = Some("Cafe".into());
    draft.recurrence = Some(
        calendar_core::recurrence::rule::RecurrenceRule::new(
            calendar_core::recurrence::rule::Frequency::Weekly,
        )
        .to_mirror(),
    );
    assert_eq!(
        event_draft_used_fields(draft),
        HashSet::from([
            EventField::Title,
            EventField::Timing,
            EventField::Notes,
            EventField::Location,
            EventField::Recurrence
        ])
    );
}

#[test]
fn a_patch_touches_only_the_fields_it_changes() {
    let empty = event_patch_diff(event(), event());
    assert!(event_patch_touched_fields(empty).is_empty());
    let original = event();
    let mut edited = original.clone();
    edited.title = "Renamed".into();
    edited.notes = Some("n".into());
    let patch = event_patch_diff(original, edited);
    assert_eq!(
        event_patch_touched_fields(patch),
        HashSet::from([EventField::Title, EventField::Notes])
    );
}

#[test]
fn an_invalid_draft_timing_or_rule_fails_with_the_write_error_of_the_library() {
    use crate::values::write::WriteError;
    assert_eq!(validate_event_draft(draft()), Ok(()));
    let mut backwards = draft();
    backwards.timing = timing(1_700_003_600, 1_700_000_000);
    assert_eq!(
        validate_event_draft(backwards.clone()),
        Err(FfiError::Write(WriteError::Invalid(
            "end must be after start".into()
        )))
    );
    assert_eq!(
        validate_event_timing(backwards.timing),
        Err(FfiError::Write(WriteError::Invalid(
            "end must be after start".into()
        )))
    );
    // An all-day timing must be midnight in its zone.
    let mut all_day = timing(1_700_000_000, 1_700_086_400);
    all_day.is_all_day = true;
    assert!(matches!(
        validate_event_timing(all_day),
        Err(FfiError::Write(WriteError::Invalid(_)))
    ));
    assert_eq!(validate_event_timing(timing(0, 60)), Ok(()));

    let mut rule: crate::values::model::RecurrenceRule =
        calendar_core::recurrence::rule::RecurrenceRule::new(
            calendar_core::recurrence::rule::Frequency::Weekly,
        )
        .to_mirror();
    assert_eq!(validate_recurrence_rule(rule.clone()), Ok(()));
    rule.interval = 0;
    assert_eq!(
        validate_recurrence_rule(rule.clone()),
        Err(FfiError::Write(WriteError::Invalid(
            "recurrence interval must be at least 1".into()
        )))
    );
    let mut daily_with_weekdays = rule;
    daily_with_weekdays.interval = 1;
    daily_with_weekdays.frequency = crate::values::model::Frequency::Daily;
    daily_with_weekdays.weekdays = vec![crate::values::model::WeekdayOccurrence {
        weekday: crate::values::model::Weekday::Monday,
        ordinal: None,
    }];
    assert!(matches!(
        validate_recurrence_rule(daily_with_weekdays),
        Err(FfiError::Write(WriteError::Unsupported { .. }))
    ));
}
