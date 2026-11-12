//! Port of `Support/Fakes.swift`: a lock-protected box, a throwaway directory, the fixture directory and `poll`.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A lock-protected mutable value for sharing state between a test and the tasks it starts.
pub(crate) struct LockedBox<T> {
    value: Mutex<T>,
}

impl<T> LockedBox<T> {
    pub(crate) fn new(value: T) -> LockedBox<T> {
        LockedBox {
            value: Mutex::new(value),
        }
    }

    pub(crate) fn get(&self) -> T
    where
        T: Clone,
    {
        self.value.lock().unwrap().clone()
    }

    pub(crate) fn set(&self, new_value: T) {
        *self.value.lock().unwrap() = new_value;
    }

    pub(crate) fn mutate<R>(&self, body: impl FnOnce(&mut T) -> R) -> R {
        body(&mut self.value.lock().unwrap())
    }
}

/// A throwaway directory for store tests (`TempDir.make`). Unlike Swift's, it is removed when dropped.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// A fresh directory.
    pub(crate) fn make() -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "TimeTugEngineTests-{}",
            uuid::Uuid::new_v4().as_hyphenated()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }

    /// A fresh directory holding a copy of a fixture under its own name (`TempDir.make(copying:)`).
    pub(crate) fn make_copying(fixture: &str) -> TempDir {
        let dir = TempDir::make();
        std::fs::copy(Fixtures::path(fixture), dir.path().join(fixture)).unwrap();
        dir
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Synthetic JSON files written by the Mac encoders (`fixtures/engine/`, moved there from the Swift
/// package's `Tests/TimeTugEngineTests/Fixtures` in phase 3 task 10).
pub(crate) enum Fixtures {}

impl Fixtures {
    pub(crate) fn directory() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/engine")
    }

    pub(crate) fn path(name: &str) -> PathBuf {
        Fixtures::directory().join(name)
    }
}

/// Polls `condition` until it holds: yields, and after the first thousand checks also sleeps a millisecond between
/// checks, so a slow machine gets real time. Gives up after `timeout` seconds of wall-clock time and fails the test
/// naming `what`. The bound only limits a hang; it never decides an ordering.
pub(crate) async fn poll<F, Fut>(what: impl Fn() -> String, timeout: f64, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    let mut checks: u64 = 0;
    loop {
        if condition().await {
            return;
        }
        checks += 1;
        if Instant::now() >= deadline {
            break;
        }
        tokio::task::yield_now().await;
        if checks > 1_000 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
    panic!(
        "timed out after {} s waiting for {}",
        timeout as i64,
        what()
    );
}
