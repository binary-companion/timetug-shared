//! Port of `EngineSupport.swift` and the blocking helpers of `Support.swift`: a temporary data directory, the test config,
//! a collector for the messages an engine delivers, and the waits. The engine runs on its own threads, so these tests are
//! plain blocking tests.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::c_interface::rust_host::{self, Handle, Sink};

/// A fresh, empty data directory.
pub(crate) fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("tt-c-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A `testing: true` config in `directory` (a new temporary directory when `None`), as JSON text.
pub(crate) fn test_config(directory: Option<PathBuf>, extra: Value) -> String {
    let directory = directory.unwrap_or_else(temporary_directory);
    let mut object = Map::new();
    object.insert("v".into(), json!(1));
    object.insert(
        "dataDirectory".into(),
        json!(directory.to_str().unwrap().to_string()),
    );
    object.insert("platform".into(), json!("linux"));
    object.insert("testing".into(), json!(true));
    object.insert("testClockStart".into(), json!("2026-10-16T13:00:00.000Z"));
    object.insert("timeZone".into(), json!("Europe/Berlin"));
    if let Value::Object(extra) = extra {
        for (key, value) in extra {
            object.insert(key, value);
        }
    }
    Value::Object(object).to_string()
}

pub(crate) fn default_config() -> String {
    test_config(None, Value::Null)
}

type Hook = Arc<dyn Fn(&str) + Send + Sync>;

/// Collects every message an engine delivers.
pub(crate) struct MessageCollector {
    stored: Mutex<Vec<String>>,
    /// Runs inside the callback, on the delivery thread, after the message is recorded.
    hook: Mutex<Option<Hook>>,
}

impl MessageCollector {
    pub(crate) fn new() -> Arc<MessageCollector> {
        Arc::new(MessageCollector {
            stored: Mutex::new(Vec::new()),
            hook: Mutex::new(None),
        })
    }

    fn messages_guard(&self) -> MutexGuard<'_, Vec<String>> {
        self.stored.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn messages(&self) -> Vec<String> {
        self.messages_guard().clone()
    }

    pub(crate) fn count(&self) -> usize {
        self.messages_guard().len()
    }

    pub(crate) fn set_hook(&self, hook: impl Fn(&str) + Send + Sync + 'static) {
        *self.hook.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(hook));
    }

    fn receive(&self, text: &str) {
        self.messages_guard().push(text.to_string());
        let hook = self
            .hook
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook {
            hook(text);
        }
    }

    /// Parsed messages in delivery order.
    pub(crate) fn envelopes(&self) -> Vec<Value> {
        self.messages()
            .iter()
            .filter_map(|text| serde_json::from_str(text).ok())
            .collect()
    }

    pub(crate) fn types(&self) -> Vec<String> {
        self.envelopes()
            .iter()
            .map(|e| e["type"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    pub(crate) fn sink(self: &Arc<Self>) -> Arc<Sink> {
        let collector = self.clone();
        Arc::new(move |text: &str| collector.receive(text))
    }

    /// The payload of the newest `state` message.
    pub(crate) fn last_state(&self) -> Option<Value> {
        self.envelopes()
            .into_iter()
            .rfind(|e| e["type"] == "state")
            .map(|e| e["payload"].clone())
    }
}

/// Creates a test engine delivering into `collector`; the collector must outlive the engine.
pub(crate) fn create_test_engine(
    collector: &Arc<MessageCollector>,
    config: &str,
) -> Option<Handle> {
    rust_host::create(Some(config), Some(collector.sink()))
}

pub(crate) fn send(handle: Handle, json: &str) {
    rust_host::send(handle, Some(json));
}

/// Waits (in real time) until `condition` holds; panics after `timeout` seconds.
pub(crate) fn poll_for(what: &str, timeout: f64, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout} s waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(crate) fn poll_sync(what: &str, condition: impl FnMut() -> bool) {
    poll_for(what, 10.0, condition);
}

/// Waits until `collector` has received nothing new for `quiet` seconds (a test engine on a manual clock goes quiet once
/// its commands are handled).
pub(crate) fn settle(collector: &MessageCollector, quiet: f64, timeout: f64) {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    let mut count = collector.count();
    let mut since = Instant::now();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        let now = collector.count();
        if now != count {
            count = now;
            since = Instant::now();
        } else if since.elapsed().as_secs_f64() >= quiet {
            return;
        }
    }
    panic!("the engine did not go quiet within {timeout} s");
}
