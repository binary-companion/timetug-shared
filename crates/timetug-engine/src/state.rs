//! Port of `EngineState.swift`.

use std::collections::BTreeMap;

use calendar_core::connection::Connection;
use calendar_core::source_types::SourceNotice;
use timetug_core::agenda::day_agenda::DayAgenda;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::scheduler::ScheduledTakeover;
use timetug_core::{CalendarInfo, InferenceStatus, SourceStatus, TimeTugCalendarEvent};

use crate::accounts::account_manager::Status;
use crate::settings::EngineSettings;

/// An immutable-by-convention snapshot of everything a front end renders. `revision` increases on every publish.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineState {
    pub revision: u64,
    pub agenda: DayAgenda,
    pub events: Vec<TimeTugCalendarEvent>,
    pub calendars: Vec<CalendarInfo>,
    pub statuses: BTreeMap<String, SourceStatus>,
    pub notices: BTreeMap<String, Vec<SourceNotice>>,
    pub source_names: BTreeMap<String, String>,
    pub candidates: BTreeMap<String, Vec<TimeTugCalendarEvent>>,
    pub accounts: Vec<Connection>,
    pub account_status: Status,
    pub settings: EngineSettings,
    pub inference_status: InferenceStatus,
    pub ledger: TakeoverLedger,
    pub next_takeover: Option<ScheduledTakeover>,
    pub takeover_visible: bool,
    /// What "add account" offers, in display order (CalDAV last); system-permission kinds are not listed.
    pub available_account_kinds: Vec<AccountKindInfo>,
}

impl EngineState {
    /// Swift's `static let empty`.
    pub fn empty() -> EngineState {
        EngineState {
            revision: 0,
            agenda: DayAgenda::empty(),
            events: Vec::new(),
            calendars: Vec::new(),
            statuses: BTreeMap::new(),
            notices: BTreeMap::new(),
            source_names: BTreeMap::new(),
            candidates: BTreeMap::new(),
            accounts: Vec::new(),
            account_status: Status::empty(),
            settings: EngineSettings::default(),
            inference_status: InferenceStatus::Disabled,
            ledger: TakeoverLedger::new(),
            next_takeover: None,
            takeover_visible: false,
            available_account_kinds: Vec::new(),
        }
    }
}

/// `AccountKindInfo.Authorization`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authorization {
    Oauth,
    Password,
    System,
}

/// One choice for "add account". A kind the platform supports but this build cannot use (for example Google
/// without an OAuth client) is listed with `is_configured` false so the host can explain why it is unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountKindInfo {
    pub id: String,
    pub display_name: String,
    pub authorization: Authorization,
    pub is_configured: bool,
}

impl AccountKindInfo {
    pub fn new(
        id: impl Into<String>,
        display_name: impl Into<String>,
        authorization: Authorization,
        is_configured: bool,
    ) -> AccountKindInfo {
        AccountKindInfo {
            id: id.into(),
            display_name: display_name.into(),
            authorization,
            is_configured,
        }
    }
}
