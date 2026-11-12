//! Port of `EngineCommand.swift`.

use std::fmt;

use timetug_core::TimeTugCalendarEvent;
use timetug_core::model::merge_types::MergedMember;

use crate::settings::EngineSettings;

/// A platform or system condition the host reports to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    Wake,
    ClockChanged,
    TimeZoneChanged,
    DayChanged,
    NetworkReturned,
    DisplayChanged,
}

/// Everything a front end can ask the engine to do. Events are identified by `TimeTugCalendarEvent::id()`.
// A command is built once and moved once into the engine; boxing the two-event variants would only change how the
// plain data of the Swift enum is spelled.
#[allow(clippy::large_enum_variant)]
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
    /// Swift's `@Sendable (inout EngineSettings) -> Void`. The engine applies it once; the protocol crate builds it
    /// from a settings patch.
    ApplySettings(Box<dyn FnOnce(&mut EngineSettings) + Send>),
    SettingsChangedExternally,
    SetCountdownVisible(bool),
    /// `on_screen` false means the front end fell back to a toast.
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
    /// Settings' "Test tug": a sample takeover. The host supplies the display strings (the engine produces none).
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

/// The settings closure has no `Debug`; it prints as `ApplySettings(..)`.
impl fmt::Debug for EngineCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineCommand::Start => f.write_str("Start"),
            EngineCommand::Refresh => f.write_str("Refresh"),
            EngineCommand::AddAccount { kind_id } => f
                .debug_struct("AddAccount")
                .field("kind_id", kind_id)
                .finish(),
            EngineCommand::RemoveAccount { connection_id } => f
                .debug_struct("RemoveAccount")
                .field("connection_id", connection_id)
                .finish(),
            EngineCommand::ReauthorizeAccount { connection_id } => f
                .debug_struct("ReauthorizeAccount")
                .field("connection_id", connection_id)
                .finish(),
            EngineCommand::CancelAuthorization => f.write_str("CancelAuthorization"),
            EngineCommand::ApplySettings(_) => f.write_str("ApplySettings(..)"),
            EngineCommand::SettingsChangedExternally => f.write_str("SettingsChangedExternally"),
            EngineCommand::SetCountdownVisible(visible) => {
                f.debug_tuple("SetCountdownVisible").field(visible).finish()
            }
            EngineCommand::TakeoverPresented {
                event_id,
                on_screen,
            } => f
                .debug_struct("TakeoverPresented")
                .field("event_id", event_id)
                .field("on_screen", on_screen)
                .finish(),
            EngineCommand::TakeoverClosed { event_id } => f
                .debug_struct("TakeoverClosed")
                .field("event_id", event_id)
                .finish(),
            EngineCommand::Join { event_id } => {
                f.debug_struct("Join").field("event_id", event_id).finish()
            }
            EngineCommand::Snooze { event_id, seconds } => f
                .debug_struct("Snooze")
                .field("event_id", event_id)
                .field("seconds", seconds)
                .finish(),
            EngineCommand::Dismiss { event_id } => f
                .debug_struct("Dismiss")
                .field("event_id", event_id)
                .finish(),
            EngineCommand::TestTug { .. } => f.write_str("TestTug { .. }"),
            EngineCommand::Merge(a, b) => f
                .debug_tuple("Merge")
                .field(&a.id())
                .field(&b.id())
                .finish(),
            EngineCommand::Unmerge(event) => f.debug_tuple("Unmerge").field(&event.id()).finish(),
            EngineCommand::Separate { event, members } => f
                .debug_struct("Separate")
                .field("event", &event.id())
                .field("members", &members.len())
                .finish(),
            EngineCommand::ForgetCorrections => f.write_str("ForgetCorrections"),
            EngineCommand::System(event) => f.debug_tuple("System").field(event).finish(),
            EngineCommand::Stop => f.write_str("Stop"),
        }
    }
}
