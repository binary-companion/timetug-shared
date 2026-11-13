//! Port of `MessageCatalog.swift`.

use serde::{Deserialize, Serialize};

use crate::envelope::ProtocolVersion;
use crate::service_messages::ServiceMessages;

/// Every message type of protocol v1, in one place: the documentation table and the golden-fixture test both read these
/// lists, so a type cannot be added without a fixture.
pub enum MessageCatalog {}

impl MessageCatalog {
    /// Message types the host sends to the engine.
    pub const HOST_TO_ENGINE: [&'static str; 20] = [
        "start",
        "refresh",
        "account.add",
        "account.reconnect",
        "account.remove",
        "account.cancel",
        "settings.apply",
        "countdown.visible",
        "takeover.presented",
        "takeover.closed",
        "takeover.join",
        "takeover.dismiss",
        "takeover.snooze",
        "takeover.test",
        "dedup.merge",
        "dedup.unmerge",
        "dedup.separate",
        "dedup.forget",
        "system.event",
        "test.advanceClock",
    ];

    /// Message types the engine sends to the host (not the `service.*` requests, which `MessageCatalog::services`
    /// lists).
    pub const ENGINE_TO_HOST: [&'static str; 7] = [
        "ready",
        "state",
        "event.takeoverDue",
        "event.takeoverWithdrawn",
        "event.widgetSnapshot",
        "event.joinRequested",
        "event.diagnostic",
    ];

    /// `MessageCatalog.services`: the `service.*` request types.
    pub fn services() -> &'static [&'static str] {
        &ServiceMessages::TYPES
    }

    /// Host to engine, engine to host, then services.
    pub fn all() -> Vec<&'static str> {
        let mut all: Vec<&'static str> = Self::HOST_TO_ENGINE.to_vec();
        all.extend(Self::ENGINE_TO_HOST);
        all.extend(Self::services().iter().copied());
        all
    }
}

/// `WireReady.Capabilities`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireReadyCapabilities {
    /// True when the engine offers the system calendar (EventKit on the Mac) as a source.
    pub system_calendar: bool,
    /// True when the engine can use a text model to judge duplicates.
    pub text_generation: bool,
    /// True when the engine will ask the host for access tokens (`service.token.*`). Omitted when false, so the `ready`
    /// message of a host that did not opt in is the 1.7.0 text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_tokens: Option<bool>,
}

impl WireReadyCapabilities {
    /// Swift's two-argument initializer: `externalTokens` stays `nil`.
    pub fn new(system_calendar: bool, text_generation: bool) -> WireReadyCapabilities {
        WireReadyCapabilities {
            system_calendar,
            text_generation,
            external_tokens: None,
        }
    }

    /// Swift's three-argument initializer.
    pub fn with_external_tokens(
        system_calendar: bool,
        text_generation: bool,
        external_tokens: Option<bool>,
    ) -> WireReadyCapabilities {
        WireReadyCapabilities {
            system_calendar,
            text_generation,
            external_tokens,
        }
    }
}

/// Payload of the first message the engine sends after `start`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireReady {
    /// Version of the engine build.
    pub engine_version: String,
    /// Protocol versions the engine speaks.
    pub protocol_versions: Vec<i64>,
    /// The platform the engine was created for: `windows`, `linux` or `macos`.
    pub platform: String,
    /// What this engine instance can do.
    pub capabilities: WireReadyCapabilities,
}

impl WireReady {
    /// Swift's `init` with `protocolVersions` at its default (`ProtocolVersion.supported`).
    pub fn new(
        engine_version: impl Into<String>,
        platform: impl Into<String>,
        capabilities: WireReadyCapabilities,
    ) -> WireReady {
        WireReady {
            engine_version: engine_version.into(),
            protocol_versions: ProtocolVersion::SUPPORTED.to_vec(),
            platform: platform.into(),
            capabilities,
        }
    }
}
