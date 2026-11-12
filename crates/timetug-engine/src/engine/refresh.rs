//! `Engine.swift` lines 238-294 (Refresh and apply), 523-588 (Inference) and 657-673 (System events).

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use calendar_core::time::add_seconds;
use timetug_core::dedup::adjudication::DuplicateAdjudicator;
use timetug_core::{CalendarSnapshot, CalendarSource};

use super::{ActivityEnd, Engine, EngineInner, TaskSlot, sleep_until_cancellable, source_ids};
use crate::accounts::cancellation::is_task_cancelled;
use crate::activity::Kind;
use crate::command::SystemEvent;
use crate::diagnostic::Level;

impl Engine {
    // MARK: - Refresh and apply

    pub(super) async fn refresh(&self, source_ids: Option<BTreeSet<String>>) {
        let _refresh = ActivityEnd::begin(&self.activity, Kind::Refresh);
        let (now, lead_time) = {
            let inner = self.lock();
            (self.host.clock.now(), inner.settings.takeover.lead_time)
        };
        let weak = self.weak.clone();
        let on_publication = move |snapshot: CalendarSnapshot| {
            let weak = weak.clone();
            Box::pin(async move {
                if let Some(engine) = weak.upgrade() {
                    engine.apply_snapshot(snapshot);
                }
            }) as Pin<Box<dyn Future<Output = ()> + Send>>
        };
        // await point (Engine.swift:243): no lock is held while the sources are read.
        let result = self
            .store
            .refresh(now, lead_time, source_ids.as_ref(), Some(&on_publication))
            .await;
        let mut inner = self.lock();
        self.apply(&mut inner, result);
        // Inference never blocks a refresh.
        self.start_inference_resolution(&mut inner);
    }

    /// `apply` as the store's `onPublication` hop onto the engine.
    fn apply_snapshot(&self, snapshot: CalendarSnapshot) {
        let mut inner = self.lock();
        self.apply(&mut inner, snapshot);
    }

    /// Publishes a snapshot: ledger bookkeeping, launch acknowledgement, re-arm, state and widget snapshot. A snapshot no
    /// newer than the current one is dropped (a late publication from an older refresh). After `stop` nothing is applied:
    /// a refresh still in flight must not arm or persist anything.
    pub(super) fn apply(&self, inner: &mut EngineInner, new_snapshot: CalendarSnapshot) {
        if inner.stopped || new_snapshot.revision <= inner.snapshot.revision {
            return;
        }
        inner.snapshot = new_snapshot;
        let now = self.host.clock.now();
        if inner.ledger.prune(now) {
            self.persist_ledger(inner);
        }
        let to_acknowledge = inner.launch_acknowledgement.events_to_acknowledge(
            &inner.snapshot,
            now,
            Engine::LAUNCH_GRACE,
        );
        if !to_acknowledge.is_empty() {
            for event in &to_acknowledge {
                inner.ledger.mark_fired(event, now);
            }
            self.diagnose(
                inner,
                Level::Info,
                "takeover",
                &format!(
                    "acknowledged {} meeting(s) already underway at launch",
                    to_acknowledge.len()
                ),
            );
            self.persist_ledger(inner);
        }
        self.rearm(inner);
        self.publish(inner, true);
        self.emit_widget_snapshot(inner);
    }

    /// Called by the account manager whenever the source set changes.
    pub(super) async fn apply_sources(&self, sources: Vec<Arc<dyn CalendarSource>>) {
        {
            let mut inner = self.lock();
            let now = self.host.clock.now();
            inner
                .launch_acknowledgement
                .register_sources(&source_ids(&sources), now);
            // await point (Engine.swift:273): `AccountManager::status` is synchronous in Rust.
            inner.account_status = self.account_manager.status();
        }
        // await point (Engine.swift:274): `store.setSources` is synchronous in Rust.
        self.store.set_sources(sources);
        // await point (Engine.swift:275)
        self.refresh(None).await;
    }

    pub(super) fn arm_periodic_refresh(&self, inner: &mut EngineInner) {
        if let Some(task) = inner.periodic_task.take() {
            task.cancel();
        }
        if inner.stopped {
            return;
        }
        let clock = self.host.clock.clone();
        let activity = self.activity.clone();
        let weak = self.weak.clone();
        // The first deadline is fixed when arming, not when the task first runs.
        let mut deadline = add_seconds(clock.now(), Engine::PERIODIC_REFRESH);
        inner.periodic_task = Some(TaskSlot::spawn(async move {
            while !is_task_cancelled() {
                // await point (Engine.swift:286)
                if sleep_until_cancellable(&clock, deadline).await.is_err() {
                    return;
                }
                let Some(engine) = weak.upgrade() else {
                    activity.wake_handled();
                    return;
                };
                // await point (Engine.swift:288)
                engine.refresh(None).await;
                deadline = add_seconds(clock.now(), Engine::PERIODIC_REFRESH);
                activity.wake_handled();
            }
        }));
    }

    // MARK: - Inference

    /// Starts inference resolution in `resolve_task` without blocking the caller (inference never blocks a refresh).
    /// While one runs, it runs once more instead of a second one starting.
    pub(super) fn start_inference_resolution(&self, inner: &mut EngineInner) {
        inner.resolve_requested = true;
        if inner.resolve_task.is_some() || inner.stopped {
            return;
        }
        self.activity.begin(Kind::Resolution);
        let end = ActivityEnd::for_begun(&self.activity, Kind::Resolution);
        let weak = self.weak.clone();
        inner.resolve_task = Some(TaskSlot::spawn(async move {
            let _end = end;
            if let Some(engine) = weak.upgrade() {
                // await point (Engine.swift:534)
                engine.run_resolution().await;
            }
        }));
    }

    async fn run_resolution(&self) {
        loop {
            {
                let mut inner = self.lock();
                // The check and the `resolveTask = nil` of Swift's loop exit are one actor turn.
                if !(inner.resolve_requested && !is_task_cancelled()) {
                    inner.resolve_task = None;
                    return;
                }
                inner.resolve_requested = false;
            }
            // await point (Engine.swift:541)
            self.resolve_pending().await;
        }
    }

    /// Starts resolution (or joins the running one) and waits for it.
    pub(super) async fn resolve_now(&self) {
        let task = {
            let mut inner = self.lock();
            self.start_inference_resolution(&mut inner);
            inner.resolve_task.as_ref().map(TaskSlot::finished)
        };
        if let Some(task) = task {
            // await point (Engine.swift:550)
            task.wait().await;
        }
    }

    /// Port of the Mac `resolvePending`: asks the text model about look-alike pairs, at most `MAX_RESOLVE_PASSES` passes;
    /// each verdict republishes. Dedup state is persisted once, at the end, when it changed. A model that is temporarily
    /// unavailable leaves the pairs pending for a later refresh (no retry loop).
    async fn resolve_pending(&self) {
        if let Some(adjudicator) = &self.adjudicator {
            let before = adjudicator.availability();
            // The availability is cached: the store's pending pairs were computed with the old value, so a change
            // re-snapshots (a model that came back has pairs to judge; one that left falls back to the rules).
            // await point (Engine.swift:561)
            let after = adjudicator.refresh_availability().await;
            if after != before {
                let mut inner = self.lock();
                // await points (Engine.swift:562, 563): the store is synchronous in Rust.
                inner.inference_status = self.store.inference_status();
                let snapshot = self
                    .store
                    .set_inference_enabled(inner.settings.inference_enabled, self.host.clock.now());
                self.apply(&mut inner, snapshot);
            }
        }
        // await point (Engine.swift:566)
        self.update_inference_status();
        let mut passes = 0;
        while passes < Engine::MAX_RESOLVE_PASSES && !is_task_cancelled() {
            let now = self.host.clock.now();
            // await point (Engine.swift:569): no lock is held while the model is asked.
            let Some(updated) = self.store.resolve_pending(now).await else {
                break;
            };
            {
                let mut inner = self.lock();
                self.apply(&mut inner, updated);
            }
            passes += 1;
        }
        // await points (Engine.swift:573, 574)
        self.update_inference_status();
        let mut inner = self.lock();
        self.persist_dedup(&mut inner);
    }

    fn update_inference_status(&self) {
        let mut inner = self.lock();
        // await point (Engine.swift:578): `store.inferenceStatus` is synchronous in Rust.
        let status = self.store.inference_status();
        if status == inner.inference_status {
            return;
        }
        inner.inference_status = status;
        self.publish(&mut inner, false);
    }

    pub(super) async fn set_inference(&self, enabled: bool) {
        {
            let mut inner = self.lock();
            // await point (Engine.swift:585): `store.setInferenceEnabled` is synchronous in Rust.
            let snapshot = self
                .store
                .set_inference_enabled(enabled, self.host.clock.now());
            self.apply(&mut inner, snapshot);
        }
        // await point (Engine.swift:586)
        self.resolve_now().await;
    }

    // MARK: - System events

    /// Timers alone are not trusted (port of the Mac `observeSystemEvents` and network monitor).
    pub(super) async fn handle_system_event(&self, event: SystemEvent) {
        match event {
            SystemEvent::Wake
            | SystemEvent::ClockChanged
            | SystemEvent::TimeZoneChanged
            | SystemEvent::DayChanged => {
                // await point (Engine.swift:663): `store.setCalendar` is synchronous in Rust.
                self.store.set_zone((self.host.zone)());
                // await point (Engine.swift:664)
                self.refresh(None).await;
            }
            SystemEvent::NetworkReturned => {
                // A source poll backs off after a failure, so refresh as soon as the network returns.
                // await point (Engine.swift:667)
                self.refresh(None).await;
            }
            SystemEvent::DisplayChanged => {
                // The front end re-lays out from a fresh state; nothing is refetched.
                let mut inner = self.lock();
                self.publish(&mut inner, false);
            }
        }
    }
}
