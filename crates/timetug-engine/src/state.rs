//! Port of `EngineState.swift`.

use std::collections::BTreeMap;

use calendar_core::connection::Connection;
use calendar_core::source_types::SourceNotice;
use jiff::Timestamp;
use timetug_core::agenda::day_agenda::DayAgenda;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::scheduler::ScheduledTakeover;
use timetug_core::{CalendarInfo, InferenceStatus, SourceStatus, TimeTugCalendarEvent};

use crate::accounts::account_manager::Status;
use crate::settings::EngineSettings;

/// An immutable-by-convention snapshot of everything a front end renders. `revision` increases on every publish.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineState {
    /// Starts at 0 and increases with every published state.
    pub revision: u64,
    /// The day agenda: what is on now and next, built from `events`.
    pub agenda: DayAgenda,
    /// Deduplicated events from all sources.
    pub events: Vec<TimeTugCalendarEvent>,
    /// Every calendar the sources offer.
    pub calendars: Vec<CalendarInfo>,
    /// Health of each source, keyed by source id.
    pub statuses: BTreeMap<String, SourceStatus>,
    /// Quiet hints per source, keyed by source id; a source with none has no entry.
    pub notices: BTreeMap<String, Vec<SourceNotice>>,
    /// Display name of each source, keyed by source id.
    pub source_names: BTreeMap<String, String>,
    /// Look-alike events kept separate (event id to the other events), offered for a manual merge.
    pub candidates: BTreeMap<String, Vec<TimeTugCalendarEvent>>,
    /// The stored accounts.
    pub accounts: Vec<Connection>,
    /// Sign-in progress and errors for the accounts.
    pub account_status: Status,
    /// The current settings.
    pub settings: EngineSettings,
    /// Whether the text model is available and working.
    pub inference_status: InferenceStatus,
    /// Takeovers already fired, snoozed or dismissed.
    pub ledger: TakeoverLedger,
    /// The takeover the engine has armed, if any; `None` while one is on screen.
    pub next_takeover: Option<ScheduledTakeover>,
    /// True while a takeover is shown.
    pub takeover_visible: bool,
    /// The start of the next meeting that still qualifies for a takeover (`Scheduler.next` over the events, the settings
    /// and the ledger at the time of publication), whether or not a takeover is on screen. Unlike `next_takeover` it is
    /// not cleared while one is outstanding, so a host can keep its "meeting soon" icon. Not part of the wire contract
    /// (additive: the C interface and the golden fixtures do not carry it).
    pub next_pending_start: Option<Timestamp>,
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
            next_pending_start: None,
            available_account_kinds: Vec::new(),
        }
    }
}

/// `AccountKindInfo.Authorization`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authorization {
    /// Signs in through the browser (OAuth).
    Oauth,
    /// Signs in with a user name and password the host collects.
    Password,
    /// Needs only the system's permission; no account is added.
    System,
}

/// One choice for "add account". A kind the platform supports but this build cannot use (for example Google
/// without an OAuth client) is listed with `is_configured` false so the host can explain why it is unavailable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountKindInfo {
    /// Connector kind id, as passed to `EngineCommand::AddAccount`.
    pub id: String,
    /// Name to show the user.
    pub display_name: String,
    /// How the kind signs in.
    pub authorization: Authorization,
    /// False when the platform supports the kind but this build cannot use it, for example Google without an OAuth
    /// client.
    pub is_configured: bool,
}

impl AccountKindInfo {
    /// Creates a kind entry.
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
