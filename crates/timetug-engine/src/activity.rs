//! Port of `EngineActivity.swift`.
//!
//! Lock-protected bookkeeping of the engine's in-flight work, readable from any context. Tests use it to wait for
//! real quiescence instead of a fixed number of yields; the engine never reads it. Swift's `internal` is `pub` here,
//! so a host crate's tests can read the engine's counters through `Engine::activity`.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// `EngineActivity.Kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A `send` that has not returned.
    Command,
    /// A `refresh(sourceIDs:)` that has not returned.
    Refresh,
    /// An inference resolution task that has not ended.
    Resolution,
    /// An account manager message yielded to the engine and not yet handled.
    AccountMessage,
    /// A source-signal follow-up pass in the refresh coordinator.
    Coordinator,
}

impl Kind {
    /// `CaseIterable.allCases`, in declaration order.
    pub const ALL: [Kind; 5] = [
        Kind::Command,
        Kind::Refresh,
        Kind::Resolution,
        Kind::AccountMessage,
        Kind::Coordinator,
    ];

    /// The Swift raw value.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Command => "command",
            Kind::Refresh => "refresh",
            Kind::Resolution => "resolution",
            Kind::AccountMessage => "accountMessage",
            Kind::Coordinator => "coordinator",
        }
    }
}

#[derive(Default)]
struct Counters {
    counts: BTreeMap<Kind, i64>,
    handled_wakes: i64,
    emitted: i64,
    signals: i64,
    stopped: bool,
}

#[derive(Default)]
pub struct EngineActivity {
    state: Mutex<Counters>,
}

impl EngineActivity {
    pub fn new() -> EngineActivity {
        EngineActivity::default()
    }

    fn lock(&self) -> MutexGuard<'_, Counters> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn begin(&self, kind: Kind) {
        *self.lock().counts.entry(kind).or_insert(0) += 1;
    }

    pub fn end(&self, kind: Kind) {
        *self.lock().counts.entry(kind).or_insert(0) -= 1;
    }

    /// A clock sleep the engine armed returned normally and its work (or its decision to do nothing) finished.
    pub fn wake_handled(&self) {
        self.lock().handled_wakes += 1;
    }

    pub fn event_emitted(&self) {
        self.lock().emitted += 1;
    }

    pub fn signal_received(&self) {
        self.lock().signals += 1;
    }

    pub fn mark_stopped(&self) {
        self.lock().stopped = true;
    }

    pub fn wakes_handled(&self) -> i64 {
        self.lock().handled_wakes
    }

    pub fn events_emitted(&self) -> i64 {
        self.lock().emitted
    }

    pub fn signals_received(&self) -> i64 {
        self.lock().signals
    }

    /// No command, refresh, resolution, account message or coordinator pass in flight. After `stop` only commands
    /// and refreshes count (the cancelled background tasks may never report back).
    pub fn is_idle(&self) -> bool {
        let state = self.lock();
        let active: &[Kind] = if state.stopped {
            &[Kind::Command, Kind::Refresh]
        } else {
            &Kind::ALL
        };
        active
            .iter()
            .all(|kind| state.counts.get(kind).copied().unwrap_or(0) == 0)
    }

    pub fn summary(&self) -> String {
        let state = self.lock();
        let parts: Vec<String> = Kind::ALL
            .iter()
            .map(|kind| {
                format!(
                    "{}={}",
                    kind.as_str(),
                    state.counts.get(kind).copied().unwrap_or(0)
                )
            })
            .collect();
        format!(
            "{} wakesHandled={} emitted={} signals={} stopped={}",
            parts.join(" "),
            state.handled_wakes,
            state.emitted,
            state.signals,
            state.stopped
        )
    }
}
