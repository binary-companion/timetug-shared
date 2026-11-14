//! `Engine.swift` lines 674-705 (UI ticks) and 730-773 (Publication).

use timetug_core::widget::snapshot::WidgetSnapshot;

use super::{Engine, EngineInner, TaskSlot, sleep_until_cancellable};
use crate::diagnostic::{EngineDiagnostic, Level};
use crate::event::EngineEvent;
use crate::scheduling::ui_tick_policy::UITickPolicy;
use crate::state::EngineState;

impl Engine {
    // MARK: - UI ticks

    pub(super) fn set_countdown_visible(&self, inner: &mut EngineInner, visible: bool) {
        if visible == inner.countdown_visible {
            return;
        }
        inner.countdown_visible = visible;
        self.arm_tick(inner);
    }

    /// Arms one sleep for the next UI wake (port of the Mac `updateUI` tick): agenda boundaries always, and every
    /// countdown step while a countdown is visible. Re-armed on every publish.
    fn arm_tick(&self, inner: &mut EngineInner) {
        if let Some(task) = inner.tick_task.take() {
            task.cancel();
        }
        inner.tick_generation = inner.tick_generation.wrapping_add(1);
        if inner.stopped {
            return;
        }
        let zone = (self.host.zone)();
        let Some(wake) = UITickPolicy::next(
            self.host.clock.now(),
            inner.countdown_visible,
            inner.agenda.next.as_ref(),
            &inner.snapshot.events,
            inner.settings.takeover.lead_time,
            &zone,
        ) else {
            return;
        };
        let clock = self.host.clock.clone();
        let generation = inner.tick_generation;
        let activity = self.activity.clone();
        let weak = self.weak.clone();
        inner.tick_task = Some(TaskSlot::spawn(async move {
            // await point (Engine.swift:693)
            if sleep_until_cancellable(&clock, wake.date).await.is_err() {
                return;
            }
            if let Some(engine) = weak.upgrade() {
                // await point (Engine.swift:694): `tick` is one lock hold.
                engine.tick(wake.rebuild_agenda, generation);
            }
            activity.wake_handled();
        }));
    }

    /// A countdown step republishes the cached agenda; a boundary rebuilds it.
    fn tick(&self, rebuild_agenda: bool, generation: u64) {
        let mut inner = self.lock();
        if generation != inner.tick_generation {
            return;
        }
        inner.tick_task = None;
        self.publish(&mut inner, rebuild_agenda);
    }

    // MARK: - Publication

    /// Builds and sends a new state, then re-arms the UI tick. `rebuild_agenda` false keeps the agenda (a countdown tick,
    /// a status change). The only place that sends on the `watch` channel.
    pub(super) fn publish(&self, inner: &mut EngineInner, rebuild_agenda: bool) {
        if rebuild_agenda {
            inner.agenda = timetug_core::agenda::day_agenda::DayAgenda::make(
                &inner.snapshot.events,
                &inner.settings.takeover,
                self.host.clock.now(),
                &(self.host.zone)(),
            );
        }
        inner.revision = inner.revision.wrapping_add(1);
        inner.state = EngineState {
            revision: inner.revision,
            agenda: inner.agenda.clone(),
            events: inner.snapshot.events.clone(),
            calendars: inner.snapshot.calendars.clone(),
            statuses: inner.snapshot.statuses.clone(),
            notices: inner.snapshot.notices.clone(),
            source_names: inner.snapshot.source_names.clone(),
            candidates: inner.snapshot.candidates.clone(),
            accounts: inner.account_status.accounts.clone(),
            account_status: inner.account_status.clone(),
            settings: inner.settings.clone(),
            inference_status: inner.inference_status.clone(),
            ledger: inner.ledger.clone(),
            next_takeover: inner.next_takeover.clone(),
            takeover_visible: inner.takeover_visible,
            available_account_kinds: inner.available_account_kinds.clone(),
        };
        if let Some(sender) = &inner.state_tx {
            sender.send_replace(inner.state.clone());
        }
        self.arm_tick(inner);
    }

    pub(super) fn emit_widget_snapshot(&self, inner: &mut EngineInner) {
        let snapshot = WidgetSnapshot::make(
            &inner.snapshot.widget_events,
            &inner.snapshot.calendars,
            &inner.settings.takeover,
            self.host.clock.now(),
            &(self.host.zone)(),
        );
        self.emit(inner, EngineEvent::WidgetSnapshot(snapshot));
    }

    pub(super) fn emit(&self, inner: &mut EngineInner, event: EngineEvent) {
        if !inner.event_senders.is_empty() {
            self.activity.event_emitted();
        }
        // A closed receiver is pruned on the next send (Swift removes a subscriber when its stream terminates).
        inner
            .event_senders
            .retain(|sender| sender.send(event.clone()).is_ok());
    }

    /// Never after `stop`: a fired entry written then would suppress a takeover the user never saw.
    pub(super) fn persist_ledger(&self, inner: &mut EngineInner) {
        if inner.stopped {
            return;
        }
        let _ = self.ledger_store.save(&inner.ledger, self.host.clock.now());
    }

    /// Engine-level diagnostics go to the host sink and, as `.diagnostic`, to `events`. Never include titles.
    pub(super) fn diagnose(
        &self,
        inner: &mut EngineInner,
        level: Level,
        category: &str,
        message: &str,
    ) {
        let diagnostic = EngineDiagnostic::new(level, category, message);
        (self.host.diagnostics)(diagnostic.clone());
        self.emit(inner, EngineEvent::Diagnostic(diagnostic));
    }
}
