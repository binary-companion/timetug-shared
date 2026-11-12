//! Port of the nested types of `AccountManager.swift` (lines 20 to 32). Phase 3 task 2 adds the `AccountManager` itself
//! to this file; task 1 needs `Status` because `EngineState` holds one.

use std::collections::BTreeMap;

use calendar_core::connection::{Connection, ConnectionId};

/// `AccountManager.WaitingStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitingStyle {
    Browser,
    InApp,
}

/// `AccountManager.Status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub accounts: Vec<Connection>,
    pub is_working: bool,
    pub build_failures: BTreeMap<ConnectionId, String>,
    pub error_message: Option<String>,
    pub waiting_style: WaitingStyle,
}

impl Default for Status {
    fn default() -> Status {
        Status {
            accounts: Vec::new(),
            is_working: false,
            build_failures: BTreeMap::new(),
            error_message: None,
            waiting_style: WaitingStyle::Browser,
        }
    }
}

impl Status {
    /// Swift's `static let empty`.
    pub fn empty() -> Status {
        Status::default()
    }
}
