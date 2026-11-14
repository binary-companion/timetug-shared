//! Mirrors of what the host sends to the engine: commands and system events.
#![allow(missing_docs)]

use timetug_engine::command::{
    EngineCommand as EngineEngineCommand, SystemEvent as EngineSystemEvent,
};

use super::model::{MergedMember, TimeTugCalendarEvent};
use super::settings::SettingsPatch;
use super::{ToEngine, mirror_unit_enum};

mirror_unit_enum!(SystemEvent <=> EngineSystemEvent { Wake, ClockChanged, TimeZoneChanged, DayChanged, NetworkReturned, DisplayChanged });

/// Everything a front end can ask the engine to do. `ApplySettings` takes a patch instead of a closure.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum EngineCommand {
    Start,
    Refresh,
    AddAccount {
        kind_id: String,
    },
    RemoveAccount {
        connection_id: String,
    },
    ReauthorizeAccount {
        connection_id: String,
    },
    CancelAuthorization,
    ApplySettings(SettingsPatch),
    SettingsChangedExternally,
    SetCountdownVisible(bool),
    TakeoverPresented {
        event_id: String,
        on_screen: bool,
    },
    TakeoverClosed {
        event_id: String,
    },
    Join {
        event_id: String,
    },
    Snooze {
        event_id: String,
        seconds: f64,
    },
    Dismiss {
        event_id: String,
    },
    TestTug {
        title: String,
        calendar_title: String,
    },
    Merge(TimeTugCalendarEvent, TimeTugCalendarEvent),
    Unmerge(TimeTugCalendarEvent),
    Separate {
        event: TimeTugCalendarEvent,
        members: Vec<MergedMember>,
    },
    ForgetCorrections,
    System(SystemEvent),
    Stop,
}

impl ToEngine for EngineCommand {
    type Engine = EngineEngineCommand;
    fn to_engine(self) -> EngineEngineCommand {
        match self {
            EngineCommand::Start => EngineEngineCommand::Start,
            EngineCommand::Refresh => EngineEngineCommand::Refresh,
            EngineCommand::AddAccount { kind_id } => EngineEngineCommand::AddAccount { kind_id },
            EngineCommand::RemoveAccount { connection_id } => {
                EngineEngineCommand::RemoveAccount { connection_id }
            }
            EngineCommand::ReauthorizeAccount { connection_id } => {
                EngineEngineCommand::ReauthorizeAccount { connection_id }
            }
            EngineCommand::CancelAuthorization => EngineEngineCommand::CancelAuthorization,
            EngineCommand::ApplySettings(patch) => {
                EngineEngineCommand::ApplySettings(Box::new(move |settings| patch.apply(settings)))
            }
            EngineCommand::SettingsChangedExternally => {
                EngineEngineCommand::SettingsChangedExternally
            }
            EngineCommand::SetCountdownVisible(visible) => {
                EngineEngineCommand::SetCountdownVisible(visible)
            }
            EngineCommand::TakeoverPresented {
                event_id,
                on_screen,
            } => EngineEngineCommand::TakeoverPresented {
                event_id,
                on_screen,
            },
            EngineCommand::TakeoverClosed { event_id } => {
                EngineEngineCommand::TakeoverClosed { event_id }
            }
            EngineCommand::Join { event_id } => EngineEngineCommand::Join { event_id },
            EngineCommand::Snooze { event_id, seconds } => {
                EngineEngineCommand::Snooze { event_id, seconds }
            }
            EngineCommand::Dismiss { event_id } => EngineEngineCommand::Dismiss { event_id },
            EngineCommand::TestTug {
                title,
                calendar_title,
            } => EngineEngineCommand::TestTug {
                title,
                calendar_title,
            },
            EngineCommand::Merge(first, second) => {
                EngineEngineCommand::Merge(first.to_engine(), second.to_engine())
            }
            EngineCommand::Unmerge(event) => EngineEngineCommand::Unmerge(event.to_engine()),
            EngineCommand::Separate { event, members } => EngineEngineCommand::Separate {
                event: event.to_engine(),
                members: members.to_engine(),
            },
            EngineCommand::ForgetCorrections => EngineEngineCommand::ForgetCorrections,
            EngineCommand::System(event) => EngineEngineCommand::System(event.to_engine()),
            EngineCommand::Stop => EngineEngineCommand::Stop,
        }
    }
}
