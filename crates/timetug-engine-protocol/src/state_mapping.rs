//! Port of `StateMapping.swift`: engine and Core types to wire DTOs. This is the one file where the protocol crate names
//! Core and engine types in its public API (the architecture check allow-lists it). Nothing here composes a display
//! string.

use calendar_core::source_types::SourceNoticeKind;
use serde::Serialize;
use serde_json::Value;
use timetug_core::agenda::day_agenda::State as AgendaState;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::widget::snapshot::WidgetSnapshot;
use timetug_core::{
    CalendarInfo, InferenceStatus, MergeProvenance, SourceStatus, TimeTugCalendarEvent,
};
use timetug_engine::accounts::account_manager::WaitingStyle;
use timetug_engine::diagnostic::EngineDiagnostic;
use timetug_engine::event::EngineEvent;
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::{Authorization, EngineState};

use crate::envelope::{Envelope, MessageId};
use crate::wire_types::*;

impl WireEvent {
    /// Swift's `WireEvent.init(_ event:)`.
    pub fn new(event: &TimeTugCalendarEvent) -> WireEvent {
        let (start, end, display_start, start_date, end_date) = match event.all_day_dates() {
            Some((first, end_exclusive)) => (
                None,
                None,
                None,
                Some(WireDates::date_string(first.year, first.month, first.day)),
                Some(WireDates::date_string(
                    end_exclusive.year,
                    end_exclusive.month,
                    end_exclusive.day,
                )),
            ),
            None => (
                Some(WireDates::string(event.start())),
                Some(WireDates::string(event.end())),
                event.display_start.map(WireDates::string),
                None,
                None,
            ),
        };
        WireEvent {
            id: event.id(),
            source_id: event.source_id.clone(),
            source_event_id: event.source_event_id().to_string(),
            calendar_key: event.calendar_key(),
            title: event.title().to_string(),
            is_all_day: event.is_all_day(),
            start,
            end,
            start_date,
            end_date,
            display_start,
            time_zone: event.time_zone().identifier().to_string(),
            location: event.location().map(str::to_string),
            url: event.url().map(|u| u.as_str().to_string()),
            status: event.event.status.as_str().to_string(),
            conferences: event
                .conferences
                .iter()
                .map(|c| WireConference {
                    url: c.url.as_str().to_string(),
                    provider: c.provider.as_str().to_string(),
                })
                .collect(),
            other_attendee_count: event.other_attendee_count,
            response_status: event.response_status.map(|r| r.as_str().to_string()),
            // A `BTreeSet` iterates in order, which is `sorted()`.
            additional_calendar_keys: event.additional_calendar_keys.iter().cloned().collect(),
            members: event
                .merged_members
                .iter()
                .map(|m| WireMember {
                    member_id: m.content_key.clone(),
                    calendar_key: m.calendar_key.clone(),
                    start: WireDates::string(m.start),
                    end: WireDates::string(m.end),
                    details: m.details.clone(),
                })
                .collect(),
            merge_provenance: event.merge_provenance.as_ref().map(|p| match p {
                MergeProvenance::Rule => provenance("rule", None, None),
                MergeProvenance::UserConfirmed => provenance("userConfirmed", None, None),
                MergeProvenance::Inference {
                    engine_id,
                    engine_name,
                } => provenance("inference", Some(engine_id), Some(engine_name)),
            }),
        }
    }
}

fn provenance(
    kind: &str,
    engine_id: Option<&String>,
    engine_name: Option<&String>,
) -> WireProvenance {
    WireProvenance {
        kind: kind.to_string(),
        engine_id: engine_id.cloned(),
        engine_name: engine_name.cloned(),
    }
}

impl WireCalendar {
    /// Swift's `WireCalendar.init(_ info:)`.
    pub fn new(info: &CalendarInfo) -> WireCalendar {
        WireCalendar {
            key: info.key(),
            source_id: info.source_id.clone(),
            calendar_id: info.calendar_id.clone(),
            title: info.title.clone(),
            account_name: info.account_name.clone(),
            color_hex: info.color_hex().map(str::to_string),
            kind: info.kind.as_str().to_string(),
            service: info.service.as_ref().map(|s| s.raw_value().to_string()),
            provider: info.provider.as_ref().map(|p| p.raw_value().to_string()),
        }
    }
}

impl WireSourceStatus {
    /// Swift's `WireSourceStatus.init(_ status:)`.
    pub fn new(status: &SourceStatus) -> WireSourceStatus {
        let (kind, message) = match status {
            SourceStatus::Ok => ("ok", None),
            SourceStatus::NeedsPermission => ("needsPermission", None),
            SourceStatus::AuthExpired => ("authExpired", None),
            SourceStatus::Failing(message) => ("failing", Some(message.clone())),
        };
        WireSourceStatus {
            kind: kind.to_string(),
            message,
        }
    }
}

impl WireInferenceStatus {
    /// Swift's `WireInferenceStatus.init(_ status:)`.
    pub fn new(status: &InferenceStatus) -> WireInferenceStatus {
        let bare = |kind: &str| WireInferenceStatus {
            kind: kind.to_string(),
            reason: None,
            engine_id: None,
            engine_name: None,
            is_on_device: None,
        };
        match status {
            InferenceStatus::Disabled => bare("disabled"),
            InferenceStatus::NoEngine => bare("noEngine"),
            InferenceStatus::NotOnDevice => bare("notOnDevice"),
            InferenceStatus::Unavailable { reason } => WireInferenceStatus {
                reason: Some(reason.clone()),
                ..bare("unavailable")
            },
            InferenceStatus::Active(info) => WireInferenceStatus {
                engine_id: Some(info.id.clone()),
                engine_name: Some(info.display_name.clone()),
                is_on_device: Some(info.is_on_device),
                ..bare("active")
            },
        }
    }
}

impl WireSettings {
    /// Swift's `WireSettings.init(_ settings:)`.
    pub fn new(settings: &EngineSettings) -> WireSettings {
        let t = &settings.takeover;
        WireSettings {
            takeover: WireTakeoverSettings {
                lead_time: t.lead_time,
                takeover_calendar_keys: t.takeover_calendar_keys.iter().cloned().collect(),
                hidden_calendar_keys: t.hidden_calendar_keys.iter().cloned().collect(),
                require_conference_link: t.require_conference_link,
                require_other_attendees: t.require_other_attendees,
                skip_all_day_events: t.skip_all_day_events,
                enabled: t.enabled,
            },
            inference_enabled: settings.inference_enabled,
            system_calendar_enabled: settings.system_calendar_enabled,
        }
    }
}

impl WireSettingsPatch {
    /// Applies every present field; absent fields leave `settings` alone. Hiding a calendar also removes its takeover
    /// opt-in (a hidden calendar never tugs), the same invariant `TakeoverSettings` decodes with.
    pub fn apply_to(&self, settings: &mut EngineSettings) {
        if let Some(v) = self.lead_time {
            settings.takeover.lead_time = v;
        }
        if let Some(v) = &self.takeover_calendar_keys {
            settings.takeover.takeover_calendar_keys = v.iter().cloned().collect();
        }
        if let Some(v) = &self.hidden_calendar_keys {
            settings.takeover.hidden_calendar_keys = v.iter().cloned().collect();
        }
        if let Some(v) = self.require_conference_link {
            settings.takeover.require_conference_link = v;
        }
        if let Some(v) = self.require_other_attendees {
            settings.takeover.require_other_attendees = v;
        }
        if let Some(v) = self.skip_all_day_events {
            settings.takeover.skip_all_day_events = v;
        }
        if let Some(v) = self.enabled {
            settings.takeover.enabled = v;
        }
        if let Some(v) = self.inference_enabled {
            settings.inference_enabled = v;
        }
        if let Some(v) = self.system_calendar_enabled {
            settings.system_calendar_enabled = v;
        }
        let hidden = settings.takeover.hidden_calendar_keys.clone();
        settings
            .takeover
            .takeover_calendar_keys
            .retain(|key| !hidden.contains(key));
    }
}

impl WireState {
    /// Swift's `WireState.init(_ state:)`.
    pub fn new(state: &EngineState) -> WireState {
        let status = &state.account_status;
        WireState {
            revision: state.revision,
            agenda: WireAgenda {
                items: state
                    .agenda
                    .items
                    .iter()
                    .map(|item| WireAgendaItem {
                        event_id: item.event.id(),
                        state: agenda_state(item.state).to_string(),
                    })
                    .collect(),
                next_event_id: state.agenda.next.as_ref().map(TimeTugCalendarEvent::id),
            },
            events: state.events.iter().map(WireEvent::new).collect(),
            calendars: state.calendars.iter().map(WireCalendar::new).collect(),
            statuses: state
                .statuses
                .iter()
                .map(|(key, status)| (key.clone(), WireSourceStatus::new(status)))
                .collect(),
            notices: state
                .notices
                .iter()
                .map(|(key, notices)| {
                    (
                        key.clone(),
                        notices
                            .iter()
                            .map(|n| WireNotice {
                                kind: notice_kind(n.kind).to_string(),
                                count: n.count,
                            })
                            .collect(),
                    )
                })
                .collect(),
            source_names: state.source_names.clone(),
            candidates: state
                .candidates
                .iter()
                .map(|(key, events)| {
                    (
                        key.clone(),
                        events.iter().map(TimeTugCalendarEvent::id).collect(),
                    )
                })
                .collect(),
            accounts: state
                .accounts
                .iter()
                .map(|c| WireAccount {
                    kind_id: c.kind_id.clone(),
                    connection_id: c.connection_id.clone(),
                    display_name: c.display_name.clone(),
                    config: c.config.clone(),
                })
                .collect(),
            account_status: WireAccountStatus {
                is_working: status.is_working,
                build_failures: status.build_failures.clone(),
                error_message: status.error_message.clone(),
                waiting_style: match status.waiting_style {
                    WaitingStyle::InApp => "inApp",
                    WaitingStyle::Browser => "browser",
                }
                .to_string(),
            },
            settings: WireSettings::new(&state.settings),
            inference_status: WireInferenceStatus::new(&state.inference_status),
            ledger: WireLedger {
                count: state.ledger.key_count() as i64,
            },
            next_takeover: state.next_takeover.as_ref().map(|next| WireNextTakeover {
                event_id: next.event.id(),
                fire_at: WireDates::string(next.fire_at),
            }),
            takeover_visible: state.takeover_visible,
            available_account_kinds: state
                .available_account_kinds
                .iter()
                .map(|kind| WireAccountKind {
                    id: kind.id.clone(),
                    display_name: kind.display_name.clone(),
                    authorization: authorization(kind.authorization).to_string(),
                    is_configured: kind.is_configured,
                })
                .collect(),
        }
    }
}

fn agenda_state(state: AgendaState) -> &'static str {
    match state {
        AgendaState::Past => "past",
        AgendaState::Current => "current",
        AgendaState::Upcoming => "upcoming",
    }
}

fn notice_kind(kind: SourceNoticeKind) -> &'static str {
    match kind {
        SourceNoticeKind::UnreadableRecurrence => "unreadableRecurrence",
    }
}

fn authorization(a: Authorization) -> &'static str {
    match a {
        Authorization::Oauth => "oauth",
        Authorization::Password => "password",
        Authorization::System => "system",
    }
}

impl WireTakeoverRequest {
    /// Swift's `WireTakeoverRequest.init(_ request:, calendarTitle:)`.
    pub fn new(request: &TakeoverRequest, calendar_title: Option<String>) -> WireTakeoverRequest {
        WireTakeoverRequest {
            event: WireEvent::new(&request.event),
            has_started: request.has_started,
            join_url: request.join_url.as_ref().map(|u| u.as_str().to_string()),
            snooze_options: request.snooze_options.clone(),
            calendar_title,
        }
    }
}

impl WireWidgetSnapshot {
    /// Swift's `WireWidgetSnapshot.init(_ snapshot:)`.
    pub fn new(snapshot: &WidgetSnapshot) -> WireWidgetSnapshot {
        WireWidgetSnapshot {
            generated_at: WireDates::string(snapshot.generated_at),
            events: snapshot
                .events
                .iter()
                .map(|e| WireWidgetEvent {
                    id: e.id.clone(),
                    title: e.title.clone(),
                    start: WireDates::string(e.start),
                    end: WireDates::string(e.end),
                    is_all_day: e.is_all_day,
                    color_hex: e.color_hex.clone(),
                    join_url: e.join_url.as_ref().map(|u| u.as_str().to_string()),
                })
                .collect(),
        }
    }
}

impl WireDiagnostic {
    /// Swift's `WireDiagnostic.init(_ diagnostic:)`.
    pub fn new(diagnostic: &EngineDiagnostic) -> WireDiagnostic {
        WireDiagnostic {
            level: diagnostic.level.as_str().to_string(),
            category: diagnostic.category.clone(),
            message: diagnostic.message.clone(),
        }
    }
}

/// Swift's `JSONValue(encoding:)`: the JSON value of a DTO. Encoding a value the module's own DTOs produce cannot fail;
/// like Swift, a failure would give `null`.
pub fn json_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// An engine event as a host-bound message. Events carry no envelope id; the session adds one only to echo a rejected
/// command.
pub fn wire_message_for_event(event: &EngineEvent) -> Envelope {
    match event {
        EngineEvent::TakeoverDue {
            request,
            calendar_title,
        } => Envelope::new("event.takeoverDue").with_payload(json_value(
            &WireTakeoverRequest::new(request, calendar_title.clone()),
        )),
        EngineEvent::TakeoverWithdrawn { event_id } => Envelope::new("event.takeoverWithdrawn")
            .with_payload(serde_json::json!({ "eventID": event_id })),
        EngineEvent::WidgetSnapshot(snapshot) => Envelope::new("event.widgetSnapshot")
            .with_payload(json_value(&WireWidgetSnapshot::new(snapshot))),
        EngineEvent::JoinRequested(url) => Envelope::new("event.joinRequested")
            .with_payload(serde_json::json!({ "url": url.as_str() })),
        EngineEvent::Diagnostic(diagnostic) => wire_diagnostic(diagnostic, None),
    }
}

/// `event.diagnostic`, with the id of the command it answers when there is one.
pub fn wire_diagnostic(diagnostic: &EngineDiagnostic, id: Option<MessageId>) -> Envelope {
    let mut envelope = Envelope::new("event.diagnostic")
        .with_payload(json_value(&WireDiagnostic::new(diagnostic)));
    envelope.id = id;
    envelope
}

/// The `state` message for a snapshot.
pub fn wire_message_for_state(state: &EngineState) -> Envelope {
    Envelope::new("state").with_payload(json_value(&WireState::new(state)))
}
