//! Mirrors of the engine state, the agenda and the takeover types. The host reads these; it never sends them.
#![allow(missing_docs)]

use std::collections::HashMap;

use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_core::InferenceStatus as EngineInferenceStatus;
use timetug_core::agenda::day_agenda as agenda;
use timetug_core::dedup::adjudication::EngineInfo as EngineEngineInfo;
use timetug_core::takeover::ledger::TakeoverLedger as EngineTakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest as EngineTakeoverRequest;
use timetug_core::takeover::scheduler::ScheduledTakeover as EngineScheduledTakeover;
use timetug_engine::state as engine_state;

use super::accounts::{AccountStatus, Connection};
use super::model::{CalendarInfo, SourceNotice, SourceStatus, TimeTugCalendarEvent};
use super::settings::EngineSettings;
use super::{ToMirror, mirror_record, mirror_unit_enum};

mirror_unit_enum!(DayAgendaItemState <=> agenda::State { Past, Current, Upcoming });
mirror_record!(forward_only DayAgendaItem <=> agenda::Item { event: TimeTugCalendarEvent, state: DayAgendaItemState });
mirror_record!(forward_only DayAgenda <=> agenda::DayAgenda {
    items: Vec<DayAgendaItem>,
    next: Option<TimeTugCalendarEvent>,
});

mirror_record!(forward_only TakeoverRequest <=> EngineTakeoverRequest {
    event: TimeTugCalendarEvent,
    has_started: bool,
    join_url: Option<UrlString>,
    snooze_options: Vec<f64>,
});

mirror_record!(forward_only ScheduledTakeover <=> EngineScheduledTakeover {
    event: TimeTugCalendarEvent,
    fire_at: Timestamp,
});

/// Which events already took over. The engine owns the ledger; the host sees how many keys it holds (its persistence
/// is the engine's, in `takeover-ledger.json`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TakeoverLedger {
    pub key_count: u64,
}
impl ToMirror for EngineTakeoverLedger {
    type Mirror = TakeoverLedger;
    fn to_mirror(self) -> TakeoverLedger {
        TakeoverLedger {
            key_count: u64::try_from(self.key_count()).unwrap_or(u64::MAX),
        }
    }
}

mirror_record!(EngineInfo <=> EngineEngineInfo {
    id: String,
    display_name: String,
    is_on_device: bool,
});

/// Whether the text model is available and working.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum InferenceStatus {
    Disabled,
    NoEngine,
    Unavailable { reason: String },
    NotOnDevice,
    Active(EngineInfo),
}
impl ToMirror for EngineInferenceStatus {
    type Mirror = InferenceStatus;
    fn to_mirror(self) -> InferenceStatus {
        match self {
            EngineInferenceStatus::Disabled => InferenceStatus::Disabled,
            EngineInferenceStatus::NoEngine => InferenceStatus::NoEngine,
            EngineInferenceStatus::Unavailable { reason } => {
                InferenceStatus::Unavailable { reason }
            }
            EngineInferenceStatus::NotOnDevice => InferenceStatus::NotOnDevice,
            EngineInferenceStatus::Active(info) => InferenceStatus::Active(info.to_mirror()),
        }
    }
}

mirror_unit_enum!(Authorization <=> engine_state::Authorization { Oauth, Password, System });
mirror_record!(forward_only AccountKindInfo <=> engine_state::AccountKindInfo {
    id: String,
    display_name: String,
    authorization: Authorization,
    is_configured: bool,
});

mirror_record!(forward_only EngineState <=> engine_state::EngineState {
    revision: u64,
    agenda: DayAgenda,
    events: Vec<TimeTugCalendarEvent>,
    calendars: Vec<CalendarInfo>,
    statuses: HashMap<String, SourceStatus>,
    notices: HashMap<String, Vec<SourceNotice>>,
    source_names: HashMap<String, String>,
    candidates: HashMap<String, Vec<TimeTugCalendarEvent>>,
    accounts: Vec<Connection>,
    account_status: AccountStatus,
    settings: EngineSettings,
    inference_status: InferenceStatus,
    ledger: TakeoverLedger,
    next_takeover: Option<ScheduledTakeover>,
    takeover_visible: bool,
    next_pending_start: Option<Timestamp>,
    available_account_kinds: Vec<AccountKindInfo>,
});
