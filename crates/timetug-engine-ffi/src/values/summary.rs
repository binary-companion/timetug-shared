//! The light state the observer receives. A state with 500 events costs about 24 ms to reach a Swift observer (spike,
//! `docs/bindings-spike.md` section 4), far over the budget, so the observer gets this summary and fetches the parts that
//! are large (events, agenda, candidates) only when their revision changed.
#![allow(missing_docs)]

use std::collections::HashMap;

use jiff::Timestamp;

use timetug_engine::state::EngineState as EngineEngineState;

use super::ToMirror;
use super::accounts::{AccountStatus, Connection};
use super::model::{CalendarInfo, SourceNotice, SourceStatus, TimeTugCalendarEvent};
use super::settings::EngineSettings;
use super::state::{
    AccountKindInfo, DayAgenda, InferenceStatus, ScheduledTakeover, TakeoverLedger,
};

/// The revisions of the three large parts of a state: the revision of the state in which the part last changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartRevisions {
    pub events: u64,
    pub agenda: u64,
    pub candidates: u64,
}

impl PartRevisions {
    /// The revisions after `state` follows `previous` (which was published with `revisions`): a part keeps its revision
    /// when it is equal to the previous one.
    pub fn after(
        previous: &EngineEngineState,
        revisions: PartRevisions,
        state: &EngineEngineState,
    ) -> PartRevisions {
        PartRevisions {
            events: if previous.events == state.events {
                revisions.events
            } else {
                state.revision
            },
            agenda: if previous.agenda == state.agenda {
                revisions.agenda
            } else {
                state.revision
            },
            candidates: if previous.candidates == state.candidates {
                revisions.candidates
            } else {
                state.revision
            },
        }
    }
}

/// Everything in an `EngineState` except its three large parts, which have their own revision here and their own getter
/// on the engine (`currentEvents`, `currentAgenda`, `currentCandidates`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EngineStateSummary {
    pub revision: u64,
    pub events_revision: u64,
    pub agenda_revision: u64,
    pub candidates_revision: u64,
    pub calendars: Vec<CalendarInfo>,
    pub statuses: HashMap<String, SourceStatus>,
    pub notices: HashMap<String, Vec<SourceNotice>>,
    pub source_names: HashMap<String, String>,
    pub accounts: Vec<Connection>,
    pub account_status: AccountStatus,
    pub settings: EngineSettings,
    pub inference_status: InferenceStatus,
    pub ledger: TakeoverLedger,
    pub next_takeover: Option<ScheduledTakeover>,
    pub takeover_visible: bool,
    /// The start of the next meeting that still qualifies for a takeover, whether or not one is on screen (`next_takeover`
    /// is `None` while one is). A menu bar icon that turns "soon" ten minutes ahead reads this one.
    pub next_pending_start: Option<Timestamp>,
    pub available_account_kinds: Vec<AccountKindInfo>,
}

impl EngineStateSummary {
    /// The summary of `state`, published with the part revisions `revisions`.
    pub fn of(state: &EngineEngineState, revisions: PartRevisions) -> EngineStateSummary {
        // Clones only what the summary carries: the three large parts stay where they are.
        EngineStateSummary {
            revision: state.revision,
            events_revision: revisions.events,
            agenda_revision: revisions.agenda,
            candidates_revision: revisions.candidates,
            calendars: state.calendars.clone().to_mirror(),
            statuses: state.statuses.clone().to_mirror(),
            notices: state.notices.clone().to_mirror(),
            source_names: state.source_names.clone().to_mirror(),
            accounts: state.accounts.clone().to_mirror(),
            account_status: state.account_status.clone().to_mirror(),
            settings: state.settings.clone().to_mirror(),
            inference_status: state.inference_status.clone().to_mirror(),
            ledger: state.ledger.clone().to_mirror(),
            next_takeover: state.next_takeover.clone().to_mirror(),
            takeover_visible: state.takeover_visible,
            next_pending_start: state.next_pending_start,
            available_account_kinds: state.available_account_kinds.clone().to_mirror(),
        }
    }
}

/// The three large parts as the getters of the engine return them.
pub fn events_of(state: &EngineEngineState) -> Vec<TimeTugCalendarEvent> {
    state.events.clone().to_mirror()
}

/// The agenda of `state`.
pub fn agenda_of(state: &EngineEngineState) -> DayAgenda {
    state.agenda.clone().to_mirror()
}

/// The merge candidates of `state`.
pub fn candidates_of(state: &EngineEngineState) -> HashMap<String, Vec<TimeTugCalendarEvent>> {
    state.candidates.clone().to_mirror()
}
