//! `Engine.swift` lines 356-522 (Takeover scheduling): the ledger, the fire guard and the scheduling loop.

use calendar_core::model::{CalendarEvent, ConferenceInfo, ConferenceOrigin, ConferenceProvider};
use calendar_core::time::add_seconds;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;
use timetug_core::takeover::guard::{Decision, Reason, TakeoverGuard};
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::takeover::scheduler::Scheduler;

use super::{Engine, EngineInner, TaskSlot, sleep_until_cancellable};
use crate::diagnostic::Level;
use crate::event::EngineEvent;

impl Engine {
    // MARK: - Takeover scheduling

    /// Arms one sleep for the next takeover (port of the Mac `rearm`). Emits `takeoverWithdrawn` when the previously armed
    /// meeting is no longer scheduled at all, also while a takeover is outstanding. While one is outstanding nothing is
    /// armed (`next_takeover` is `None`); the pending meeting stays unfired in the ledger and closing the outstanding
    /// takeover re-arms it.
    pub(super) fn rearm(&self, inner: &mut EngineInner) {
        if let Some(task) = inner.fire_task.take() {
            task.cancel();
        }
        inner.fire_generation = inner.fire_generation.wrapping_add(1);
        if inner.stopped {
            return;
        }
        let previous = inner.next_takeover.take();
        let now = self.host.clock.now();
        if let Some(previous) = previous
            && !self.is_scheduled(inner, &previous.event, now)
        {
            self.emit(
                inner,
                EngineEvent::TakeoverWithdrawn {
                    event_id: previous.event.id(),
                },
            );
        }
        if inner.takeover_outstanding() {
            return;
        }
        let next = Scheduler::next(
            &inner.snapshot.events,
            &inner.settings.takeover,
            &inner.ledger,
            now,
        );
        inner.next_takeover = next.clone();
        let Some(next) = next else { return };
        let clock = self.host.clock.clone();
        let generation = inner.fire_generation;
        let activity = self.activity.clone();
        let weak = self.weak.clone();
        inner.fire_task = Some(TaskSlot::spawn(async move {
            // await point (Engine.swift:388)
            if sleep_until_cancellable(&clock, next.fire_at).await.is_err() {
                return;
            }
            if let Some(engine) = weak.upgrade() {
                // await point (Engine.swift:389): `fire_armed` is one lock hold.
                engine.fire_armed(next.event, generation);
            }
            activity.wake_handled();
        }));
    }

    /// Whether the current copy of `event` would still be armed at some point.
    fn is_scheduled(
        &self,
        inner: &EngineInner,
        event: &TimeTugCalendarEvent,
        now: Timestamp,
    ) -> bool {
        let Some(current) = inner
            .snapshot
            .events
            .iter()
            .find(|candidate| candidate.is_same_meeting(event))
        else {
            return false;
        };
        Scheduler::next(
            std::slice::from_ref(current),
            &inner.settings.takeover,
            &inner.ledger,
            now,
        )
        .is_some()
    }

    /// Runs `fire` only for the sleep armed last: a re-armed (cancelled) sleep that already woke does nothing, as an
    /// invalidated Mac timer never fired.
    fn fire_armed(&self, event: TimeTugCalendarEvent, generation: u64) {
        let mut inner = self.lock();
        if inner.stopped || generation != inner.fire_generation {
            return;
        }
        inner.fire_task = None;
        self.fire(&mut inner, &event);
    }

    /// Re-checks the ledger and the current snapshot before anything is emitted (port of the Mac `fire`): the event
    /// captured when the sleep was armed may be stale. Diagnostics carry the reason only, never the title.
    fn fire(&self, inner: &mut EngineInner, event: &TimeTugCalendarEvent) {
        if inner.stopped {
            return;
        }
        let now = self.host.clock.now();
        let decision = TakeoverGuard::evaluate(
            event,
            &inner.snapshot.events,
            &inner.settings.takeover,
            &inner.ledger,
            now,
            inner.takeover_outstanding(),
        );
        match decision {
            Decision::Suppress(Reason::OverlayVisible) => {
                // Stays pending, not marked fired; closing the visible takeover re-arms. Re-arming now clears the
                // `next_takeover` whose sleep just ended (nothing is armed while a takeover is outstanding).
                self.diagnose(
                    inner,
                    Level::Info,
                    "takeover",
                    &format!("takeover suppressed: {}", Reason::OverlayVisible.as_str()),
                );
                self.rearm(inner);
                self.publish(inner, true);
            }
            Decision::Suppress(reason) => {
                self.diagnose(
                    inner,
                    Level::Info,
                    "takeover",
                    &format!("takeover suppressed: {}", reason.as_str()),
                );
                inner.next_takeover = None;
                self.rearm(inner);
                self.publish(inner, true);
            }
            Decision::Present => {
                let current = inner
                    .snapshot
                    .events
                    .iter()
                    .find(|candidate| candidate.is_same_meeting(event))
                    .unwrap_or(event)
                    .clone();
                let reason = if inner.ledger.is_snoozed(&current) {
                    "snooze expired"
                } else if now >= add_seconds(current.start(), Engine::LAUNCH_GRACE) {
                    "late after wake"
                } else {
                    "lead time"
                };
                inner.ledger.mark_fired(&current, now);
                self.persist_ledger(inner);
                self.diagnose(
                    inner,
                    Level::Info,
                    "takeover",
                    &format!("takeover due ({reason})"),
                );
                inner.next_takeover = None;
                inner.outstanding_takeover = Some(current.clone());
                inner.outstanding_is_test = false;
                let calendar_key = current.calendar_key();
                let calendar_title = inner
                    .snapshot
                    .calendars
                    .iter()
                    .find(|calendar| calendar.key() == calendar_key)
                    .map(|calendar| calendar.title.clone());
                self.emit(
                    inner,
                    EngineEvent::TakeoverDue {
                        request: TakeoverRequest::make(&current, now),
                        calendar_title,
                    },
                );
                self.rearm(inner);
                self.publish(inner, true);
            }
        }
    }

    /// The host shows the takeover on screen, or (`on_screen` false) as a toast; either holds the next takeover. A report
    /// for any other event (a late or duplicate report after close) is ignored: it would otherwise hold the guard with
    /// nothing left to close it.
    pub(super) fn takeover_presented(
        &self,
        inner: &mut EngineInner,
        event_id: &str,
        on_screen: bool,
    ) {
        if inner
            .outstanding_takeover
            .as_ref()
            .map(TimeTugCalendarEvent::id)
            .as_deref()
            != Some(event_id)
        {
            self.diagnose(
                inner,
                Level::Warning,
                "takeover",
                "takeoverPresented for an event that is not the outstanding takeover; ignored",
            );
            return;
        }
        inner.takeover_visible = true;
        self.diagnose(
            inner,
            Level::Info,
            "takeover",
            if on_screen {
                "takeover shown on screen"
            } else {
                "takeover shown as a toast"
            },
        );
        self.publish(inner, true);
    }

    pub(super) fn takeover_closed(&self, inner: &mut EngineInner, event_id: &str) {
        if !self.matches_outstanding(inner, event_id, "takeoverClosed") {
            return;
        }
        self.close_takeover(inner);
    }

    /// Asks the host to open the meeting link (never for the test tug, as on the Mac), then closes.
    pub(super) fn join(&self, inner: &mut EngineInner, event_id: &str) {
        if !self.matches_outstanding(inner, event_id, "join") {
            return;
        }
        if !inner.outstanding_is_test
            && let Some(url) = inner
                .outstanding_takeover
                .as_ref()
                .and_then(|event| event.conference_url().cloned())
        {
            self.emit(inner, EngineEvent::JoinRequested(url));
        }
        self.close_takeover(inner);
    }

    /// Re-arms the meeting `seconds` from now (never past its end) and closes. The test tug never touches the ledger.
    pub(super) fn snooze(&self, inner: &mut EngineInner, event_id: &str, seconds: f64) {
        if !self.matches_outstanding(inner, event_id, "snooze") {
            return;
        }
        if !inner.outstanding_is_test
            && let Some(event) = inner.outstanding_takeover.clone()
        {
            inner.ledger.snooze(&event, seconds, self.host.clock.now());
        }
        self.close_takeover(inner);
    }

    pub(super) fn dismiss(&self, inner: &mut EngineInner, event_id: &str) {
        if !self.matches_outstanding(inner, event_id, "dismiss") {
            return;
        }
        self.close_takeover(inner);
    }

    /// A command for another event than the outstanding takeover is stale (a late close of an earlier takeover) and is
    /// ignored so it cannot release the guard held for the current one. Nothing outstanding, or the same id, matches
    /// (Swift: `guard let outstandingTakeover, outstandingTakeover.id != eventID else { return true }`).
    fn matches_outstanding(&self, inner: &mut EngineInner, event_id: &str, command: &str) -> bool {
        let Some(outstanding) = &inner.outstanding_takeover else {
            return true;
        };
        if outstanding.id() == event_id {
            return true;
        }
        self.diagnose(
            inner,
            Level::Warning,
            "takeover",
            &format!("{command} for an event that is not the outstanding takeover; ignored"),
        );
        false
    }

    /// Port of the Mac `closeOverlay`: clears the guard, persists, re-arms and publishes.
    fn close_takeover(&self, inner: &mut EngineInner) {
        let was_test = inner.outstanding_is_test;
        inner.takeover_visible = false;
        inner.outstanding_takeover = None;
        inner.outstanding_is_test = false;
        if !was_test {
            self.persist_ledger(inner);
        }
        self.rearm(inner);
        self.publish(inner, true);
    }

    /// Settings' "Test tug" (port of the Mac `fireTest`): a sample event that never touches the ledger and, as on the Mac,
    /// leaves the armed takeover alone (the guard holds it while the sample is outstanding). The host supplies `title` and
    /// `calendar_title`; the ids stay "test".
    pub(super) fn test_tug(&self, inner: &mut EngineInner, title: &str, calendar_title: &str) {
        let now = self.host.clock.now();
        let lead_time = inner.settings.takeover.lead_time;
        let start = add_seconds(now, lead_time);
        let mut sample = TimeTugCalendarEvent::new(
            CalendarEvent::new("test", "test", title, start, add_seconds(start, 1800.0)),
            "test",
        );
        sample.other_attendee_count = 1;
        // Swift force-unwraps this URL; the literal is valid, and `parse` accepts it.
        sample.conferences = UrlString::parse("https://meet.google.com/aaa-bbbb-ccc")
            .map(|url| {
                vec![ConferenceInfo::new(
                    url,
                    ConferenceProvider::Meet,
                    ConferenceOrigin::Structured,
                )]
            })
            .unwrap_or_default();
        inner.outstanding_takeover = Some(sample.clone());
        inner.outstanding_is_test = true;
        self.diagnose(inner, Level::Info, "takeover", "test takeover due");
        self.emit(
            inner,
            EngineEvent::TakeoverDue {
                request: TakeoverRequest::make(&sample, now),
                calendar_title: Some(calendar_title.to_string()),
            },
        );
        self.publish(inner, true);
    }
}
