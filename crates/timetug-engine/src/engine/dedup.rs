//! `Engine.swift` lines 589-624 (Dedup actions).
//!
//! Every `await store.…` of the Swift file is a synchronous call in Rust (`CalendarStore` is a `Mutex`, and Swift awaited
//! it only because actors are isolated), so these handlers hold the engine lock for their whole body: no other command
//! can run between the store call and `apply`. Swift could interleave there; the effect is only that a handler is now
//! atomic.

use timetug_core::TimeTugCalendarEvent;
use timetug_core::model::merge_types::MergedMember;

use super::{Engine, EngineInner};
use crate::diagnostic::Level;

impl Engine {
    // MARK: - Dedup actions

    pub(super) fn merge(&self, a: &TimeTugCalendarEvent, b: &TimeTugCalendarEvent) {
        let mut inner = self.lock();
        // await point (Engine.swift:592): `store.merge` is synchronous in Rust.
        let snapshot = self.store.merge(a, b, self.host.clock.now());
        // Counts only: titles are never logged.
        self.diagnose(
            &mut inner,
            Level::Info,
            "dedup",
            &format!(
                "manual merge of {} copies; {} events now shown",
                a.participants().len() + b.participants().len(),
                snapshot.events.len()
            ),
        );
        self.apply(&mut inner, snapshot);
        // await point (Engine.swift:597): `persistDedup` is synchronous in Rust.
        self.persist_dedup(&mut inner);
    }

    pub(super) fn unmerge(&self, event: &TimeTugCalendarEvent) {
        let mut inner = self.lock();
        // await point (Engine.swift:601): `store.unmerge` is synchronous in Rust.
        let snapshot = self.store.unmerge(event, self.host.clock.now());
        self.apply(&mut inner, snapshot);
        // await point (Engine.swift:602)
        self.persist_dedup(&mut inner);
    }

    pub(super) fn separate(&self, event: &TimeTugCalendarEvent, members: &[MergedMember]) {
        let mut inner = self.lock();
        // await point (Engine.swift:606): `store.separate` is synchronous in Rust.
        let snapshot = self.store.separate(members, event, self.host.clock.now());
        self.apply(&mut inner, snapshot);
        // await point (Engine.swift:607)
        self.persist_dedup(&mut inner);
    }

    /// Clears lessons and cached verdicts, then asks the model again.
    pub(super) async fn forget_corrections(&self) {
        {
            let mut inner = self.lock();
            // await point (Engine.swift:612): `store.forgetLessons` is synchronous in Rust.
            let snapshot = self.store.forget_lessons(self.host.clock.now());
            self.apply(&mut inner, snapshot);
            // await point (Engine.swift:613)
            self.persist_dedup(&mut inner);
        }
        // await point (Engine.swift:614)
        self.resolve_now().await;
    }

    /// Writes the dedup state only when it changed since the last write (or load).
    pub(super) fn persist_dedup(&self, inner: &mut EngineInner) {
        if inner.stopped {
            return;
        }
        // await point (Engine.swift:620): `store.state` is synchronous in Rust.
        let current = self.store.state();
        if inner.last_saved_dedup_state.as_ref() == Some(&current) {
            return;
        }
        if self.dedup_store.save(&current) {
            inner.last_saved_dedup_state = Some(current);
        }
    }
}
