//! Swift task cancellation, for the code that checks it by hand (`Task.isCancelled`, `Task.checkCancellation()`,
//! `withUnsafeCurrentTask { $0?.cancel() }`).
//!
//! A cancelled Swift task keeps running until it reaches a suspension point that checks. A dropped Rust future just
//! stops, so a `CancelToken` carries the flag, and the code that Swift cancelled cooperatively stays cooperative: the
//! body of an operation runs inside [`run_in_task`], which makes its token the "current task" (a `tokio` task-local),
//! and the same code reads it back with [`is_task_cancelled`] and [`check_cancellation`]. Where Swift's cancellation
//! ends a pending `await` (a sign-in waiting for a browser), `AccountManager` races that future against
//! [`CancelToken::cancelled`] and drops it, so the drop guards of conventions section 10, item 11 run.

use std::future::Future;
use std::sync::Arc;

use calendar_core::failure::{Failure, Outcome};
use tokio::sync::watch;

/// The cancellation flag of one task. Clones share the flag.
#[derive(Clone, Debug)]
pub struct CancelToken {
    flag: Arc<watch::Sender<bool>>,
}

impl Default for CancelToken {
    fn default() -> CancelToken {
        CancelToken::new()
    }
}

impl CancelToken {
    /// Creates a token that is not cancelled.
    pub fn new() -> CancelToken {
        CancelToken {
            flag: Arc::new(watch::Sender::new(false)),
        }
    }

    /// `Task.cancel()`. Idempotent.
    pub fn cancel(&self) {
        self.flag.send_replace(true);
    }

    /// `Task.isCancelled` for this task.
    pub fn is_cancelled(&self) -> bool {
        *self.flag.borrow()
    }

    /// Completes once the task is cancelled (at once when it already is).
    pub async fn cancelled(&self) {
        let mut receiver = self.flag.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

tokio::task_local! {
    static CURRENT: CancelToken;
}

/// Runs `future` as the task `token` stands for: inside it, `is_task_cancelled` and friends read `token`.
pub async fn run_in_task<F: Future>(token: CancelToken, future: F) -> F::Output {
    CURRENT.scope(token, future).await
}

/// Runs `future` in the task it is already part of, or in a new task nobody can cancel (a caller that is not a
/// `begin...` task, as when Swift code awaits an actor method directly).
pub async fn in_current_or_new_task<F: Future>(future: F) -> F::Output {
    if CURRENT.try_with(|_| ()).is_ok() {
        future.await
    } else {
        CURRENT.scope(CancelToken::new(), future).await
    }
}

/// The token of the task this code runs in, if it runs in one.
pub fn current_token() -> Option<CancelToken> {
    CURRENT.try_with(CancelToken::clone).ok()
}

/// `Task.isCancelled`.
pub fn is_task_cancelled() -> bool {
    CURRENT.try_with(CancelToken::is_cancelled).unwrap_or(false)
}

/// `try Task.checkCancellation()`.
pub fn check_cancellation() -> Outcome<()> {
    if is_task_cancelled() {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}

/// `withUnsafeCurrentTask { $0?.cancel() }`.
pub fn cancel_current_task() {
    if let Some(token) = current_token() {
        token.cancel();
    }
}
