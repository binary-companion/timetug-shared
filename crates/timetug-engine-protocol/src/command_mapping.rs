//! Port of `CommandMapping.swift`: decodes host command envelopes into engine commands.

use calendar_core::time::Zone;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use timetug_core::{MergedMember, TimeTugCalendarEvent};
use timetug_engine::command::{EngineCommand, SystemEvent};
use timetug_engine::diagnostic::{EngineDiagnostic, Level};
use timetug_engine::state::EngineState;

use crate::envelope::{Envelope, ProtocolVersion};
use crate::wire_types::WireSettingsPatch;

/// What a host command decodes to.
pub enum CommandOutcome {
    Engine(EngineCommand),
    /// `system.event` with `kind: timeZoneChanged` and a zone: the session adopts the zone, then sends the command.
    TimeZone(Zone, EngineCommand),
    TestAdvanceClock {
        seconds: f64,
    },
    /// The command was not understood or not applicable. The diagnostic has no id; the session adds the command's.
    Rejected(EngineDiagnostic),
}

/// Decodes host command envelopes into engine commands. Never fails and never panics: anything it cannot use is
/// `Rejected` with a diagnostic whose message holds no calendar content.
#[derive(Clone, Copy, Debug)]
pub struct CommandDecoder {
    testing: bool,
}

/// Swift's private `Rejection`: the message of the diagnostic.
struct Rejection(&'static str);

const INVALID_PAYLOAD: Rejection = Rejection("invalid payload");

#[derive(Deserialize)]
struct EventRef {
    #[serde(rename = "eventID")]
    event_id: String,
}

#[derive(Deserialize)]
struct Kind {
    #[serde(rename = "kindID")]
    kind_id: String,
}

#[derive(Deserialize)]
struct Connection {
    #[serde(rename = "connectionID")]
    connection_id: String,
}

#[derive(Deserialize)]
struct Visible {
    visible: bool,
}

#[derive(Deserialize)]
struct Presented {
    #[serde(rename = "eventID")]
    event_id: String,
    #[serde(rename = "onScreen")]
    on_screen: bool,
}

#[derive(Deserialize)]
struct Snooze {
    #[serde(rename = "eventID")]
    event_id: String,
    seconds: f64,
}

#[derive(Deserialize)]
struct TestTug {
    title: String,
    #[serde(rename = "calendarTitle")]
    calendar_title: String,
}

#[derive(Deserialize)]
struct Merge {
    #[serde(rename = "eventIDs")]
    event_ids: Vec<String>,
}

#[derive(Deserialize)]
struct Separate {
    #[serde(rename = "eventID")]
    event_id: String,
    #[serde(rename = "memberIDs")]
    member_ids: Vec<String>,
}

#[derive(Deserialize)]
struct System {
    kind: String,
    #[serde(default, rename = "timeZone")]
    time_zone: Option<String>,
}

#[derive(Deserialize)]
struct Advance {
    seconds: f64,
}

impl CommandDecoder {
    pub fn new(testing: bool) -> CommandDecoder {
        CommandDecoder { testing }
    }

    /// `state` is the latest published state; event ids resolve against `state.events` and the values of
    /// `state.candidates`.
    pub fn decode(&self, envelope: &Envelope, state: &EngineState) -> CommandOutcome {
        if !ProtocolVersion::is_supported(envelope.v) {
            return reject("unsupported protocol version");
        }
        match self.decode_known(envelope, state) {
            Ok(outcome) => outcome,
            Err(rejection) => reject(rejection.0),
        }
    }

    fn decode_known(
        &self,
        envelope: &Envelope,
        state: &EngineState,
    ) -> Result<CommandOutcome, Rejection> {
        let engine = |command: EngineCommand| Ok(CommandOutcome::Engine(command));
        match envelope.r#type.as_str() {
            "start" => engine(EngineCommand::Start),
            "refresh" => engine(EngineCommand::Refresh),
            "account.add" => engine(EngineCommand::AddAccount {
                kind_id: payload::<Kind>(envelope)?.kind_id,
            }),
            "account.reconnect" => engine(EngineCommand::ReauthorizeAccount {
                connection_id: payload::<Connection>(envelope)?.connection_id,
            }),
            "account.remove" => engine(EngineCommand::RemoveAccount {
                connection_id: payload::<Connection>(envelope)?.connection_id,
            }),
            "account.cancel" => engine(EngineCommand::CancelAuthorization),
            "settings.apply" => {
                let patch: WireSettingsPatch = payload(envelope)?;
                engine(EngineCommand::ApplySettings(Box::new(move |settings| {
                    patch.apply_to(settings)
                })))
            }
            "countdown.visible" => engine(EngineCommand::SetCountdownVisible(
                payload::<Visible>(envelope)?.visible,
            )),
            "takeover.presented" => {
                let p: Presented = payload(envelope)?;
                engine(EngineCommand::TakeoverPresented {
                    event_id: p.event_id,
                    on_screen: p.on_screen,
                })
            }
            "takeover.closed" => engine(EngineCommand::TakeoverClosed {
                event_id: payload::<EventRef>(envelope)?.event_id,
            }),
            "takeover.join" => engine(EngineCommand::Join {
                event_id: payload::<EventRef>(envelope)?.event_id,
            }),
            "takeover.dismiss" => engine(EngineCommand::Dismiss {
                event_id: payload::<EventRef>(envelope)?.event_id,
            }),
            "takeover.snooze" => {
                let p: Snooze = payload(envelope)?;
                if !(p.seconds.is_finite() && p.seconds >= 0.0) {
                    return Err(INVALID_PAYLOAD);
                }
                engine(EngineCommand::Snooze {
                    event_id: p.event_id,
                    seconds: p.seconds,
                })
            }
            "takeover.test" => {
                let p: TestTug = payload(envelope)?;
                engine(EngineCommand::TestTug {
                    title: p.title,
                    calendar_title: p.calendar_title,
                })
            }
            "dedup.merge" => {
                let p: Merge = payload(envelope)?;
                if p.event_ids.len() != 2 {
                    return Err(INVALID_PAYLOAD);
                }
                engine(EngineCommand::Merge(
                    event(&p.event_ids[0], state)?,
                    event(&p.event_ids[1], state)?,
                ))
            }
            "dedup.unmerge" => engine(EngineCommand::Unmerge(event(
                &payload::<EventRef>(envelope)?.event_id,
                state,
            )?)),
            "dedup.separate" => {
                let p: Separate = payload(envelope)?;
                let merged = event(&p.event_id, state)?;
                let members = p
                    .member_ids
                    .iter()
                    .map(|id| {
                        merged
                            .merged_members
                            .iter()
                            .find(|m| &m.content_key == id)
                            .cloned()
                            .ok_or(Rejection("unknown member id"))
                    })
                    .collect::<Result<Vec<MergedMember>, Rejection>>()?;
                engine(EngineCommand::Separate {
                    event: merged,
                    members,
                })
            }
            "dedup.forget" => engine(EngineCommand::ForgetCorrections),
            "system.event" => system_event(payload(envelope)?),
            "test.advanceClock" => {
                if !self.testing {
                    return Err(Rejection("test commands are disabled"));
                }
                let p: Advance = payload(envelope)?;
                if !(p.seconds.is_finite() && p.seconds >= 0.0) {
                    return Err(INVALID_PAYLOAD);
                }
                Ok(CommandOutcome::TestAdvanceClock { seconds: p.seconds })
            }
            _ => Err(Rejection("unknown command type")),
        }
    }
}

fn reject(message: &str) -> CommandOutcome {
    CommandOutcome::Rejected(EngineDiagnostic::new(Level::Warning, "command", message))
}

/// The payload as `T`; an absent payload reads as `{}`. Any decoding failure is "invalid payload".
fn payload<T: DeserializeOwned>(envelope: &Envelope) -> Result<T, Rejection> {
    let value = envelope
        .payload
        .clone()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    serde_json::from_value(value).map_err(|_| INVALID_PAYLOAD)
}

fn event(id: &str, state: &EngineState) -> Result<TimeTugCalendarEvent, Rejection> {
    if let Some(found) = state.events.iter().find(|e| e.id() == id) {
        return Ok(found.clone());
    }
    for list in state.candidates.values() {
        if let Some(found) = list.iter().find(|e| e.id() == id) {
            return Ok(found.clone());
        }
    }
    Err(Rejection("unknown event id"))
}

fn system_event(p: System) -> Result<CommandOutcome, Rejection> {
    let kind = match p.kind.as_str() {
        "wake" => SystemEvent::Wake,
        "clockChanged" => SystemEvent::ClockChanged,
        "timeZoneChanged" => SystemEvent::TimeZoneChanged,
        "dayChanged" => SystemEvent::DayChanged,
        "networkReturned" => SystemEvent::NetworkReturned,
        "displayChanged" => SystemEvent::DisplayChanged,
        _ => return Err(Rejection("unknown system event kind")),
    };
    if kind == SystemEvent::TimeZoneChanged
        && let Some(id) = p.time_zone
    {
        let zone = Zone::named(&id).ok_or(Rejection("unknown time zone"))?;
        return Ok(CommandOutcome::TimeZone(zone, EngineCommand::System(kind)));
    }
    Ok(CommandOutcome::Engine(EngineCommand::System(kind)))
}
