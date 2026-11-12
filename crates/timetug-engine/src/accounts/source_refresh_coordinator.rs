//! Port of `SourceRefreshCoordinator.swift`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use timetug_core::SourceChange;

use crate::accounts::account_manager::BoxFuture;
use crate::activity::{EngineActivity, Kind};

/// Swift's `@Sendable (Set<String>?) async -> Void`: `None` is a full refresh.
pub type RefreshFn = Arc<dyn Fn(Option<BTreeSet<String>>) -> BoxFuture<()> + Send + Sync>;

#[derive(Default)]
struct Pending {
    pending: BTreeSet<String>,
    pending_full: bool,
    /// Swift's `worker != nil`.
    worker_running: bool,
}

struct Shared {
    refresh: RefreshFn,
    activity: Option<Arc<EngineActivity>>,
    state: Mutex<Pending>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Ends the `.coordinator` activity when the worker task ends, however it ends (Swift's `defer`).
struct EndGuard(Option<Arc<EngineActivity>>);

impl Drop for EndGuard {
    fn drop(&mut self) {
        if let Some(activity) = &self.0 {
            activity.end(Kind::Coordinator);
        }
    }
}

/// Collects source notifications while one refresh is running. A burst causes one follow-up pass.
///
/// Swift's actor: the state sits behind a `std` mutex that is never held across an `await`. The worker is a spawned
/// task, so `signal` needs a `tokio` runtime.
pub struct SourceRefreshCoordinator {
    shared: Arc<Shared>,
}

impl SourceRefreshCoordinator {
    pub fn new(
        activity: Option<Arc<EngineActivity>>,
        refresh: RefreshFn,
    ) -> SourceRefreshCoordinator {
        SourceRefreshCoordinator {
            shared: Arc::new(Shared {
                refresh,
                activity,
                state: Mutex::new(Pending::default()),
            }),
        }
    }

    pub async fn signal(&self, change: SourceChange) {
        let shared = &self.shared;
        if let Some(activity) = &shared.activity {
            activity.signal_received();
        }
        {
            let mut state = shared.lock();
            if let SourceChange::EventsChanged {
                calendar_ids: None, ..
            } = change
            {
                state.pending_full = true;
            } else {
                state.pending.insert(change.source_id().to_string());
            }
            if state.worker_running {
                return;
            }
            state.worker_running = true;
        }
        if let Some(activity) = &shared.activity {
            activity.begin(Kind::Coordinator);
        }
        let worker = shared.clone();
        tokio::spawn(async move {
            let _end = EndGuard(worker.activity.clone());
            loop {
                let batch = {
                    let mut state = worker.lock();
                    if !(state.pending_full || !state.pending.is_empty()) {
                        // Swift: `worker = nil`, in the same turn as the loop check.
                        state.worker_running = false;
                        break;
                    }
                    let batch: Option<BTreeSet<String>> = if state.pending_full {
                        None
                    } else {
                        Some(state.pending.clone())
                    };
                    state.pending.clear();
                    state.pending_full = false;
                    batch
                };
                // await point (SourceRefreshCoordinator.swift:33): the state lock is released; signals arriving now
                // join the next pass.
                (worker.refresh)(batch).await;
            }
        });
    }
}
