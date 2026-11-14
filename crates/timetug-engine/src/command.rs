//! Port of `EngineCommand.swift`.

use std::fmt;

use timetug_core::TimeTugCalendarEvent;
use timetug_core::model::merge_types::MergedMember;

use crate::settings::EngineSettings;

/// A platform or system condition the host reports to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    /// The system woke from sleep.
    Wake,
    /// The system clock was changed.
    ClockChanged,
    /// The time zone changed.
    TimeZoneChanged,
    /// The calendar day rolled over.
    DayChanged,
    /// The network came back; sources that backed off after a failure are refreshed at once.
    NetworkReturned,
    /// The display configuration changed; the engine republishes state without refetching anything.
    DisplayChanged,
}

/// Everything a front end can ask the engine to do. Events are identified by `TimeTugCalendarEvent::id()`.
// A command is built once and moved once into the engine; boxing the two-event variants would only change how the
// plain data of the Swift enum is spelled.
#[allow(clippy::large_enum_variant)]
pub enum EngineCommand {
    /// Loads the saved state, builds the sources and refreshes once. Sending it twice is ignored with a warning.
    Start,
    /// Refreshes every source now.
    Refresh,
    /// Starts signing in a new account of connector kind `kind_id`.
    AddAccount {
        /// Id of the connector kind, one of `EngineState::available_account_kinds`.
        kind_id: String,
    },
    /// Removes a stored account and its source, selections and secrets.
    RemoveAccount {
        /// Id of the account's stored connection.
        connection_id: String,
    },
    /// Signs an existing account in again, for example after its authorization expired.
    ReauthorizeAccount {
        /// Id of the account's stored connection.
        connection_id: String,
    },
    /// Cancels the sign-in in progress, if any.
    CancelAuthorization,
    /// Swift's `@Sendable (inout EngineSettings) -> Void`. The engine applies it once; the protocol crate builds it
    /// from a settings patch.
    ApplySettings(Box<dyn FnOnce(&mut EngineSettings) + Send>),
    /// The host's settings storage was changed by someone else; the engine re-reads it without writing it back.
    SettingsChangedExternally,
    /// Tells the engine whether the menu-bar countdown is shown, which sets how often it republishes state.
    SetCountdownVisible(bool),
    /// `on_screen` false means the front end fell back to a toast.
    TakeoverPresented {
        /// Id of the event the takeover is for.
        event_id: String,
        /// True when the takeover is on screen, false when it was shown as a toast.
        on_screen: bool,
    },
    /// The takeover for `event_id` was closed by the user or the host.
    TakeoverClosed {
        /// Id of the event whose takeover closed.
        event_id: String,
    },
    /// The user chose Join; the engine emits `EngineEvent::JoinRequested` with the meeting link, then closes the
    /// takeover.
    Join {
        /// Id of the outstanding takeover's event.
        event_id: String,
    },
    /// Snoozes the outstanding takeover: it fires again after `seconds` (never later than the meeting's end) and
    /// closes.
    Snooze {
        /// Id of the outstanding takeover's event.
        event_id: String,
        /// Delay before the takeover fires again, in seconds.
        seconds: f64,
    },
    /// Dismisses the outstanding takeover without joining or snoozing.
    Dismiss {
        /// Id of the outstanding takeover's event.
        event_id: String,
    },
    /// Settings' "Test tug": a sample takeover. The host supplies the display strings (the engine produces none).
    TestTug {
        /// Title of the sample event.
        title: String,
        /// Calendar title shown for the sample event.
        calendar_title: String,
    },
    /// Merges two events into one on the user's request.
    Merge(TimeTugCalendarEvent, TimeTugCalendarEvent),
    /// Splits a merged event back into its members.
    Unmerge(TimeTugCalendarEvent),
    /// Keeps `members` of the merged `event` apart as separate events.
    Separate {
        /// The merged event to split.
        event: TimeTugCalendarEvent,
        /// The members that are to stay separate.
        members: Vec<MergedMember>,
    },
    /// Clears the saved corrections and cached model verdicts, then asks the model again.
    ForgetCorrections,
    /// Reports a system condition; the engine refreshes or republishes as needed.
    System(SystemEvent),
    /// Cancels background work, stops the sources and ends every event stream.
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
