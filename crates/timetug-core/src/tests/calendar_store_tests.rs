//! Port of `TimeTugCoreTests/CalendarStoreTests.swift`, including its `FakeSource` (which the other store tests use).
//!
//! Swift's `utcCalendar` and `calendar(in:)` are zones here (`CalendarStore` takes a `Zone`; see `set_zone`), and
//! `actor FakeSource` is a struct over a `Mutex` (it has no awaits of its own).

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use calendar_core::civil::{self, Unit};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::model::{CalendarProvider, CalendarService, UidScope};
use calendar_core::time::{DateInterval, add_seconds};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::dedup_support::{ev, url};
use super::support::{date, day, make_all_day, zone};
use crate::agenda::day_agenda::DayAgenda;
use crate::model::calendar_info::CalendarInfo;
use crate::model::event::TimeTugCalendarEvent;
use crate::model::takeover_settings::TakeoverSettings;
use crate::sources::{CalendarSource, SourceChange, SourceError, SourceStatus};
use crate::store::calendar_store::CalendarStore;
use crate::widget::snapshot::WidgetSnapshot;

pub(super) struct FakeSource {
    id: String,
    display_name: String,
    inner: Mutex<FakeInner>,
}

struct FakeInner {
    calendars: Outcome<Vec<CalendarInfo>>,
    events: Outcome<Vec<TimeTugCalendarEvent>>,
    requested: Vec<DateInterval>,
}

impl FakeSource {
    /// `FakeSource(id:)`.
    pub(super) fn new(id: &str) -> Arc<FakeSource> {
        Arc::new(FakeSource {
            id: id.to_string(),
            display_name: format!("Fake {id}"),
            inner: Mutex::new(FakeInner {
                calendars: Ok(Vec::new()),
                events: Ok(Vec::new()),
                requested: Vec::new(),
            }),
        })
    }

    /// `FakeSource()`: the id `fake`.
    pub(super) fn fake() -> Arc<FakeSource> {
        FakeSource::new("fake")
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `set(events:)`.
    pub(super) fn set_events(&self, events: Outcome<Vec<TimeTugCalendarEvent>>) {
        self.inner().events = events;
    }

    /// `set(calendars:)`.
    pub(super) fn set_calendars(&self, calendars: Vec<CalendarInfo>) {
        self.inner().calendars = Ok(calendars);
    }

    pub(super) fn requested_intervals(&self) -> Vec<DateInterval> {
        self.inner().requested.clone()
    }
}

#[async_trait]
impl CalendarSource for FakeSource {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn display_name(&self) -> String {
        self.display_name.clone()
    }
    async fn calendars(&self) -> Outcome<Vec<CalendarInfo>> {
        self.inner().calendars.clone()
    }
    async fn events_in(&self, interval: DateInterval) -> Outcome<Vec<TimeTugCalendarEvent>> {
        let mut inner = self.inner();
        inner.requested.push(interval);
        inner.events.clone()
    }
    fn changes(&self) -> UnboundedReceiver<SourceChange> {
        unbounded_channel().1 // `AsyncStream { $0.finish() }`: the sender is gone, the stream is finished.
    }
}

/// `CalendarStore(sources:calendar:)` with the clock the tests use for `passClock` (the Swift default reads the real
/// one; Core cannot).
pub(super) fn new_store(sources: Vec<Arc<dyn CalendarSource>>, zone_id: &str) -> CalendarStore {
    CalendarStore::new(sources, zone(zone_id), None, super::support::fixed_clock())
}

fn now() -> jiff::Timestamp {
    date("2026-09-18T10:00:00Z")
}

fn ids(events: &[TimeTugCalendarEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| e.source_event_id().to_string())
        .collect()
}

#[test]
fn fetch_window_spans_today_plus_lead_time_and_buffer() {
    let store = new_store(vec![], "UTC");
    let window = store.fetch_window(now(), 600.0);
    assert_eq!(window.start, date("2026-09-18T00:00:00Z"));
    assert_eq!(window.end, date("2026-09-19T00:15:00Z")); // midnight + 600s + 300s buffer
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn widget_horizon_includes_tomorrow_and_third_day_without_extending_takeovers() {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![
        ev("today").start("2026-09-18T11:00:00Z").build(),
        ev("tomorrow").start("2026-09-19T15:00:00Z").build(),
        ev("third").start("2026-09-20T09:00:00Z").build(),
        ev("fourth").start("2026-09-21T09:00:00Z").build(),
    ]));
    let store = new_store(sources![source], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(ids(&snapshot.events), ["today"]);
    assert_eq!(ids(&snapshot.widget_events), ["today", "tomorrow", "third"]);
    let widget = WidgetSnapshot::make(
        &snapshot.widget_events,
        &[],
        &TakeoverSettings::default(),
        now(),
        &zone("UTC"),
    );
    assert_eq!(widget.events.len(), 3);
    assert_eq!(source.requested_intervals().len(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn changing_calendar_updates_the_same_stores_day_window() {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![ev("denver").start("2026-09-17T18:00:00Z").build()]));
    let store = new_store(sources![source], "UTC");
    let instant = date("2026-09-18T02:00:00Z");
    assert!(
        store
            .refresh(instant, 60.0, None, None)
            .await
            .events
            .is_empty()
    );
    store.set_zone(zone("America/Denver")); // Swift: setCalendar(calendar(in: "America/Denver"))
    assert_eq!(
        store.fetch_window(instant, 60.0).start,
        date("2026-09-17T06:00:00Z")
    );
    assert_eq!(
        ids(&store.refresh(instant, 60.0, None, None).await.events),
        ["denver"]
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn refresh_asks_sources_for_widget_horizon() {
    let source = FakeSource::fake();
    let store = new_store(sources![source], "UTC");
    store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        source.requested_intervals().first().map(|i| i.end),
        Some(add_seconds(
            date("2026-09-21T00:00:00Z"),
            CalendarStore::SOURCE_QUERY_MARGIN
        ))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn merges_sorts_and_dedupes_across_calendars() {
    let (a, b) = (FakeSource::new("a"), FakeSource::new("b"));
    let dup1 = ev("1")
        .title("Design Review")
        .start("2026-09-18T11:00:00Z")
        .build();
    let mut dup2 = ev("9")
        .title("design review")
        .start("2026-09-18T11:00:00Z")
        .build();
    dup2.source_id = "b".to_string();
    let early = ev("2")
        .title("Standup")
        .start("2026-09-18T09:00:00Z")
        .build();
    a.set_events(Ok(vec![dup1, early]));
    b.set_events(Ok(vec![dup2]));
    let store = new_store(sources![a, b], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    let titles: Vec<&str> = snapshot.events.iter().map(|e| e.title()).collect();
    assert_eq!(titles, ["Standup", "Design Review"]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn fills_conference_url_from_detector_when_source_has_none() {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![
        ev("e1").location("https://acme.zoom.us/j/5").build(),
    ]));
    let store = new_store(sources![source], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        snapshot
            .events
            .first()
            .and_then(|e| e.conference_url())
            .and_then(|u| u.host()),
        Some("acme.zoom.us".to_string())
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn keeps_source_supplied_conference_url() {
    let structured = url("https://meet.google.com/aaa-bbbb-ccc");
    let source = FakeSource::fake();
    source.set_events(Ok(vec![
        ev("e1")
            .location("https://acme.zoom.us/j/5")
            .conf("https://meet.google.com/aaa-bbbb-ccc")
            .build(),
    ]));
    let store = new_store(sources![source], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(
        snapshot.events.first().and_then(|e| e.conference_url()),
        Some(&structured)
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn failing_source_keeps_last_good_events_and_reports_status() {
    let source = FakeSource::fake();
    source.set_events(Ok(vec![ev("e1").build()]));
    let store = new_store(sources![source], "UTC");
    store.refresh(now(), 60.0, None, None).await;

    source.set_events(Err(Failure::Host("Boom".to_string())));
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(snapshot.events.len(), 1);
    assert!(
        matches!(
            snapshot.statuses.get("fake"),
            Some(SourceStatus::Failing(_))
        ),
        "expected .failing"
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn permission_and_auth_errors_map_to_statuses() {
    let source = FakeSource::fake();
    let store = new_store(sources![source], "UTC");
    source.set_events(Err(SourceError::NeedsPermission.into()));
    assert_eq!(
        store
            .refresh(now(), 60.0, None, None)
            .await
            .statuses
            .get("fake"),
        Some(&SourceStatus::NeedsPermission)
    );
    source.set_events(Err(SourceError::AuthExpired.into()));
    assert_eq!(
        store
            .refresh(now(), 60.0, None, None)
            .await
            .statuses
            .get("fake"),
        Some(&SourceStatus::AuthExpired)
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn one_failing_source_does_not_affect_another() {
    let (bad, good) = (FakeSource::new("bad"), FakeSource::new("good"));
    bad.set_events(Err(SourceError::NeedsPermission.into()));
    let mut event = ev("e1").build();
    event.source_id = "good".to_string();
    good.set_events(Ok(vec![event]));
    let store = new_store(sources![bad, good], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(snapshot.statuses.get("good"), Some(&SourceStatus::Ok));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn drops_events_outside_fetch_window() {
    let yesterday = ev("y").start("2026-09-17T10:00:00Z").build();
    let source = FakeSource::fake();
    source.set_events(Ok(vec![yesterday, ev("t").build()]));
    let store = new_store(sources![source], "UTC");
    assert_eq!(
        ids(&store.refresh(now(), 60.0, None, None).await.events),
        ["t"]
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn snapshot_includes_calendars_and_source_names() {
    let source = FakeSource::fake();
    source.set_calendars(vec![CalendarInfo::new("fake", "cal", "Work")]);
    let store = new_store(sources![source], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    let titles: Vec<&str> = snapshot
        .calendars
        .iter()
        .map(|c| c.title.as_str())
        .collect();
    assert_eq!(titles, ["Work"]);
    assert_eq!(
        snapshot.source_names.get("fake").map(String::as_str),
        Some("Fake fake")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn duplicate_keeps_first_copy_but_records_other_calendar_keys() {
    let (a, b) = (FakeSource::new("fake"), FakeSource::new("fake2"));
    let first = ev("1").title("Sync").cal("family").build();
    let mut second = ev("2").title("Sync").cal("work").build();
    second.source_id = "fake/work-src".to_string();
    a.set_events(Ok(vec![first]));
    b.set_events(Ok(vec![second]));
    let store = new_store(sources![a, b], "UTC");
    let events = store.refresh(now(), 60.0, None, None).await.events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].calendar_key(), "fake/family");
    assert!(
        events[0]
            .additional_calendar_keys
            .contains("fake/work-src/work")
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn duplicate_fills_missing_fields_from_later_copy() {
    let (a, b) = (FakeSource::new("a"), FakeSource::new("b"));
    let first = ev("1").title("Sync").build();
    let mut second = ev("2")
        .title("Sync")
        .notes("Join https://acme.zoom.us/j/123")
        .build();
    second.source_id = "b".to_string();
    a.set_events(Ok(vec![first]));
    b.set_events(Ok(vec![second]));
    let store = new_store(sources![a, b], "UTC");
    let events = store.refresh(now(), 60.0, None, None).await.events;
    assert_eq!(events.len(), 1);
    assert!(
        events[0]
            .conference_url()
            .and_then(|u| u.host())
            .is_some_and(|h| h.ends_with("zoom.us"))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn same_calendar_imported_from_every_source_collapses_to_one_card() {
    // The user's Exchange, Gmail and iCloud calendars share a name; a hidden calendar still takes part in the merge.
    let names = ["exchange", "gmail", "cloud", "cloud2", "cloud3"];
    let mut list: Vec<Arc<dyn CalendarSource>> = Vec::new();
    for name in names {
        let source = FakeSource::new(name);
        let mut event = ev(&format!("{name}-1"))
            .title("O'Bryan Family Dinner")
            .start("2026-09-18T11:00:00Z")
            .cal(&format!("shared-{name}"))
            .build();
        event.source_id = name.to_string();
        source.set_events(Ok(vec![event]));
        source.set_calendars(vec![CalendarInfo::new(
            name,
            format!("shared-{name}"),
            "O'Bryan Shared",
        )]);
        list.push(source);
    }
    let store = new_store(list, "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(snapshot.events.len(), 1);
    let card = &snapshot.events[0];
    assert_eq!(card.merged_members.len(), 5);
    assert_eq!(card.additional_calendar_keys.len(), 4);
    assert_eq!(
        card.all_calendar_keys(),
        names.iter().map(|n| format!("{n}/shared-{n}")).collect()
    );
    // Hiding is a display concern: one visible copy keeps the merged card on the agenda.
    let mut settings = TakeoverSettings::default();
    settings.hidden_calendar_keys = names
        .iter()
        .skip(1)
        .map(|n| format!("{n}/shared-{n}"))
        .collect();
    let agenda = DayAgenda::make(&snapshot.events, &settings, now(), &zone("UTC"));
    assert_eq!(agenda.items.len(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn sources_are_queried_with_a_zone_margin() {
    let source = FakeSource::fake();
    let store = new_store(sources![source], "UTC");
    store.refresh(now(), 600.0, None, None).await;
    let window = store.fetch_window(now(), 600.0);
    let requested = source.requested_intervals().first().copied();
    assert_eq!(
        requested.map(|r| r.start),
        Some(add_seconds(
            window.start,
            -CalendarStore::SOURCE_QUERY_MARGIN
        ))
    );
    let widget_end = civil::add(
        Unit::Day,
        WidgetSnapshot::HORIZON_DAYS,
        window.start,
        &zone("UTC"),
    )
    .unwrap();
    assert_eq!(
        requested.map(|r| r.end),
        Some(add_seconds(
            window.end.max(widget_end),
            CalendarStore::SOURCE_QUERY_MARGIN
        ))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn far_zone_all_day_event_on_todays_date_is_kept() {
    // Viewer in UTC+14, calendar in UTC-11: the event's instants start after the viewer's day window ends.
    let midway = make_all_day("Pacific/Midway", day(2026, 9, 18), day(2026, 9, 19));
    let source = FakeSource::fake();
    source.set_events(Ok(vec![midway]));
    let store = new_store(sources![source], "Pacific/Kiritimati");
    let snapshot = store
        .refresh(date("2026-09-18T02:00:00Z"), 60.0, None, None) // Sep 18 16:00 there
        .await;
    assert_eq!(ids(&snapshot.events), ["d"]);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn manual_merge_works_when_one_copy_of_the_first_event_shares_a_calendar_with_the_second() {
    // The booking copy of a meeting and its placeholder sit on one shared calendar, and the booking is also
    // copied onto other calendars. The user says they are one meeting: the same-calendar copy must not veto it.
    let zoom = "https://acme.zoom.us/j/1";
    let elsewhere = ev("1")
        .title("Consultation with Michael")
        .cal("personal")
        .notes("n")
        .conf(zoom)
        .build();
    let on_shared = ev("2")
        .title("Consultation with Michael")
        .cal("shared")
        .notes("n")
        .conf(zoom)
        .build();
    let placeholder = ev("3")
        .title("Scott: Careerminds consultation")
        .cal("shared")
        .others(0)
        .build();
    let source = FakeSource::fake();
    source.set_events(Ok(vec![elsewhere, on_shared, placeholder]));
    let store = new_store(sources![source], "UTC");
    let before = store.refresh(now(), 60.0, None, None).await;
    assert_eq!(before.events.len(), 2);
    let after = store.merge(&before.events[0], &before.events[1], now());
    assert_eq!(after.events.len(), 1);
    assert_eq!(
        after.events[0].merge_provenance,
        Some(crate::model::merge_types::MergeProvenance::UserConfirmed)
    );
    assert_eq!(after.events[0].merged_members.len(), 3);
    // and it sticks on the next refresh
    assert_eq!(store.refresh(now(), 60.0, None, None).await.events.len(), 1);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_store_fills_service_and_provider_from_the_calendar_info() {
    let source = FakeSource::fake();
    let mut info = CalendarInfo::new("fake", "cal", "Work");
    info.service = Some(CalendarService::event_kit());
    info.provider = Some(CalendarProvider::microsoft());
    source.set_calendars(vec![info]);
    source.set_events(Ok(vec![
        ev("1")
            .title("A")
            .uid("x")
            .scope(UidScope::Provider)
            .build(),
    ]));
    let store = new_store(sources![source], "UTC");
    let snapshot = store.refresh(now(), 60.0, None, None).await;
    assert!(
        snapshot.events[0].calendar_service.as_deref() == Some("eventkit")
            && snapshot.events[0].calendar_provider.as_deref() == Some("microsoft")
    );
    assert_eq!(
        snapshot.events[0].uid_match_key(),
        Some("eventkit|microsoft|x".to_string())
    );
}
