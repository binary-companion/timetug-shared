//! Port of `EngineSystemEventTests.swift`.

use calendar_core::civil::start_of_day;
use calendar_core::time::{Zone, add_seconds};
use jiff::Timestamp;
use timetug_core::agenda::day_agenda::State as AgendaState;
use timetug_core::widget::snapshot::WidgetSnapshot;
use timetug_core::{CalendarStore, SourceStatus};

use super::support::engine_harness::{EngineHarness, sample};
use crate::clock::EngineClock;
use crate::command::{EngineCommand, SystemEvent};

fn tokyo() -> Zone {
    Zone::named("Asia/Tokyo").unwrap()
}

/// The start of the interval the store asks sources for, for "now" in `zone`.
fn query_start(zone: &Zone, now: Timestamp) -> Timestamp {
    add_seconds(start_of_day(now, zone), -CalendarStore::SOURCE_QUERY_MARGIN)
}

async fn started(events: Vec<calendar_core::model::CalendarEvent>) -> EngineHarness {
    let h = EngineHarness::new(events).await;
    h.engine.send(EngineCommand::Start).await;
    h.wait_for_state(|s| s.statuses.get("fake-c1") == Some(&SourceStatus::Ok))
        .await;
    h.settle().await;
    h
}

// Scenario 5
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn calendar_events_adopt_the_host_calendar_then_refresh() {
    for event in [
        SystemEvent::Wake,
        SystemEvent::ClockChanged,
        SystemEvent::TimeZoneChanged,
        SystemEvent::DayChanged,
    ] {
        let h = started(vec![sample("e1", 600.0)]).await;
        let base = h.c1().events_calls();
        assert_eq!(
            h.c1().last_interval().map(|i| i.start),
            Some(query_start(&EngineHarness::utc(), h.clock.now())),
            "{event:?}"
        );

        h.host_zone.set(tokyo());
        h.engine.send(EngineCommand::System(event)).await;
        assert_eq!(h.c1().events_calls(), base + 1, "{event:?}");
        assert_eq!(
            h.c1().last_interval().map(|i| i.start),
            Some(query_start(&tokyo(), h.clock.now())),
            "{event:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn network_return_refreshes_without_changing_the_calendar() {
    let h = started(vec![sample("e1", 600.0)]).await;
    let base = h.c1().events_calls();
    h.host_zone.set(tokyo());
    h.engine
        .send(EngineCommand::System(SystemEvent::NetworkReturned))
        .await;
    assert_eq!(h.c1().events_calls(), base + 1);
    assert_eq!(
        h.c1().last_interval().map(|i| i.start),
        Some(query_start(&EngineHarness::utc(), h.clock.now()))
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn display_change_republishes_without_refreshing() {
    let h = started(vec![sample("e1", 600.0)]).await;
    let base = h.c1().events_calls();
    let before = h.engine.current_state();
    h.engine
        .send(EngineCommand::System(SystemEvent::DisplayChanged))
        .await;
    let after = h.engine.current_state();
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.agenda, before.agenda);
    h.settle().await;
    assert_eq!(h.c1().events_calls(), base);
}

// Scenario 7
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn countdown_ticks_republish_every_minute_with_the_same_agenda() {
    let h = started(vec![sample("e1", 3600.0)]).await;
    let start = h.engine.current_state();

    // Hidden: the next wake is the "soon" window at +3000, so a minute passes without a publish.
    h.clock.advance(60.0).await;
    h.settle().await;
    assert_eq!(h.engine.current_state().revision, start.revision);

    h.engine
        .send(EngineCommand::SetCountdownVisible(true))
        .await;
    for minute in 1..=2u64 {
        h.clock.advance(60.0).await;
        let state = h
            .wait_for_state(|s| s.revision > start.revision + minute - 1)
            .await;
        h.settle().await;
        assert_eq!(h.engine.current_state().revision, start.revision + minute);
        assert_eq!(state.agenda, start.agenda);
    }

    h.engine
        .send(EngineCommand::SetCountdownVisible(false))
        .await;
    let hidden = h.engine.current_state().revision;
    h.clock.advance(60.0).await;
    h.settle().await;
    assert_eq!(h.engine.current_state().revision, hidden);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_tick_at_an_event_boundary_rebuilds_the_agenda() {
    let h = started(vec![sample("e1", 600.0)]).await;
    // No takeover publishes in the way: only ticks publish from here on.
    h.engine
        .send(EngineCommand::ApplySettings(Box::new(|s| {
            s.takeover.enabled = false;
        })))
        .await;
    assert_eq!(
        h.engine
            .current_state()
            .agenda
            .items
            .first()
            .map(|i| i.state),
        Some(AgendaState::Upcoming)
    );

    // +540: the agenda lead boundary publishes; the meeting is still upcoming.
    let before_lead = h.engine.current_state().revision;
    h.clock.advance(540.0).await;
    let lead = h.wait_for_state(|s| s.revision > before_lead).await;
    assert_eq!(
        lead.agenda.items.first().map(|i| i.state),
        Some(AgendaState::Upcoming)
    );
    h.settle().await;
    let before_start = h.engine.current_state().revision;

    // +600: the meeting starts, and the tick rebuilds the agenda.
    h.clock.advance(60.0).await;
    let started = h.wait_for_state(|s| s.revision > before_start).await;
    assert_eq!(
        started.agenda.items.first().map(|i| i.state),
        Some(AgendaState::Current)
    );
    assert!(started.agenda.next.is_none());
}

// Scenario 8
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_widget_snapshot_is_emitted_for_every_applied_snapshot_even_when_unchanged() {
    let h = started(vec![sample("e1", 600.0)]).await;
    let state = h.engine.current_state();
    let expected = WidgetSnapshot::make(
        &state.events,
        &state.calendars,
        &state.settings.takeover,
        h.clock.now(),
        &EngineHarness::utc(),
    );
    let before = h.widget_snapshots().len();
    assert!(before > 0);
    assert_eq!(h.widget_snapshots().last(), Some(&expected));

    h.engine.send(EngineCommand::Refresh).await;
    // Unchanged events are still emitted (the host decides whether to reload timelines); the count is not asserted
    // exactly, only that one more arrived and that it equals the previous one.
    h.wait_until("the snapshot after the refresh", || {
        h.widget_snapshots().len() > before
    })
    .await;
    h.settle().await;
    assert_eq!(h.widget_snapshots().last(), Some(&expected));
}
