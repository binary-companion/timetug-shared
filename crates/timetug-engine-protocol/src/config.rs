//! Port of `Config.swift`: the `tt_engine_create` configuration.

use std::collections::BTreeMap;

use calendar_core::time::Zone;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::{ProtocolVersion, value_as_i64};
use crate::wire_types::WireDates;

/// `WireConfig.ConnectorClient`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorClient {
    #[serde(rename = "clientID")]
    pub client_id: String,
    /// Google desktop clients also have a secret (not confidential for an installed app); Microsoft has none.
    #[serde(
        default,
        rename = "clientSecret",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_secret: Option<String>,
}

impl ConnectorClient {
    pub fn new(client_id: impl Into<String>, client_secret: Option<String>) -> ConnectorClient {
        ConnectorClient {
            client_id: client_id.into(),
            client_secret,
        }
    }
}

/// `WireConfig.Invalid`. Swift's `notJSON` is `NotJson`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("the configuration is not valid JSON of the expected shape")]
    NotJson,
    #[error("unsupported configuration version {0}")]
    UnsupportedVersion(i64),
    #[error("the data directory is empty")]
    EmptyDataDirectory,
    #[error("unknown platform {0:?}")]
    UnknownPlatform(String),
    #[error("unknown time zone {0:?}")]
    UnknownTimeZone(String),
    #[error("invalid test clock start {0:?}")]
    InvalidTestClockStart(String),
}

/// The `tt_engine_create` configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireConfig {
    pub v: i64,
    pub data_directory: String,
    /// `windows`, `linux` or `macos`.
    pub platform: String,
    /// OAuth client ids by kind family (`google`, `microsoft`).
    pub connectors: BTreeMap<String, ConnectorClient>,
    pub text_generation: bool,
    /// True when the host answers the `service.token.*` requests (it supplies access tokens from the operating system's
    /// own account service). Absent in the JSON means false; without it the engine offers no brokered kind.
    pub external_tokens: bool,
    /// An IANA zone id; `None` means the process zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    pub testing: bool,
    /// With `testing`: the manual clock's first instant (an ISO 8601 UTC string); `None` starts it at the real time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_clock_start: Option<String>,
}

/// The decoded shape: Swift's hand-written `init(from:)` reads every optional member with `decodeIfPresent ?? default`,
/// so a JSON `null` and a missing member both give the default, and a wrongly typed one fails the whole decode.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    v: Value,
    data_directory: String,
    platform: String,
    connectors: Option<BTreeMap<String, ConnectorClient>>,
    text_generation: Option<bool>,
    external_tokens: Option<bool>,
    time_zone: Option<String>,
    testing: Option<bool>,
    test_clock_start: Option<String>,
}

impl WireConfig {
    pub const PLATFORMS: [&'static str; 3] = ["windows", "linux", "macos"];

    /// Swift's `init(v: 1, dataDirectory:, platform:)` with every other argument at its default.
    pub fn new(data_directory: impl Into<String>, platform: impl Into<String>) -> WireConfig {
        WireConfig {
            v: 1,
            data_directory: data_directory.into(),
            platform: platform.into(),
            connectors: BTreeMap::new(),
            text_generation: false,
            external_tokens: false,
            time_zone: None,
            testing: false,
            test_clock_start: None,
        }
    }

    /// The same, with the opt-in for host-supplied access tokens (Swift's second initializer).
    #[must_use]
    pub fn with_external_tokens(mut self, external_tokens: bool) -> WireConfig {
        self.external_tokens = external_tokens;
        self
    }

    /// Decodes and validates. Returns `Invalid` (never a decoding error type) so callers can map it to one result. The
    /// order is Swift's: not JSON, unsupported version, empty data directory, unknown platform, unknown time zone,
    /// invalid test clock start.
    pub fn parse(json: &str) -> Result<WireConfig, Invalid> {
        let raw: Raw = serde_json::from_str(json).map_err(|_| Invalid::NotJson)?;
        let v = value_as_i64(&raw.v).ok_or(Invalid::NotJson)?;
        let config = WireConfig {
            v,
            data_directory: raw.data_directory,
            platform: raw.platform,
            connectors: raw.connectors.unwrap_or_default(),
            text_generation: raw.text_generation.unwrap_or(false),
            external_tokens: raw.external_tokens.unwrap_or(false),
            time_zone: raw.time_zone,
            testing: raw.testing.unwrap_or(false),
            test_clock_start: raw.test_clock_start,
        };
        if !ProtocolVersion::is_supported(config.v) {
            return Err(Invalid::UnsupportedVersion(config.v));
        }
        if config.data_directory.is_empty() {
            return Err(Invalid::EmptyDataDirectory);
        }
        if !Self::PLATFORMS.contains(&config.platform.as_str()) {
            return Err(Invalid::UnknownPlatform(config.platform));
        }
        if let Some(zone) = &config.time_zone
            && Zone::named(zone).is_none()
        {
            return Err(Invalid::UnknownTimeZone(zone.clone()));
        }
        if let Some(start) = &config.test_clock_start
            && WireDates::unix_nanoseconds(start).is_none()
        {
            return Err(Invalid::InvalidTestClockStart(start.clone()));
        }
        Ok(config)
    }
}
