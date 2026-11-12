//! Port of `Support.swift` (`Fixture`, `Conformance`) and `GoldenMessages.swift`, plus the small helpers the Rust tests
//! share.

pub(crate) mod conformance;
pub(crate) mod fixture;
pub(crate) mod golden_messages;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use jiff::Timestamp;

/// A timestamp from whole Unix seconds.
pub(crate) fn ts(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

/// A throwaway directory, removed when dropped.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(crate) fn make() -> TempDir {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "TimeTugEngineProtocolTests-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
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
