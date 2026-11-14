//! `EngineConfig`: everything a host says about itself when it creates the engine. Connector registration is
//! configuration (clients, which kinds), never code: hosts do not construct connector kinds.
use std::path::PathBuf;

use calendar_core::time::Zone;
use timetug_engine::connector_setup::ConnectorSetupConfig;

use crate::errors::FfiError;
use crate::values::accounts::Platform;

/// Google's desktop OAuth client (the token exchange needs the secret as well as the id).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GoogleClient {
    /// The client id.
    pub id: String,
    /// The client secret.
    pub secret: String,
}

/// Microsoft's OAuth client (public client: an id only).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MicrosoftClient {
    /// The client id.
    pub id: String,
}

/// The most days of iCalendar subscription history or future the engine keeps; a longer window is a mistake.
pub const MAX_RETENTION_DAYS: u32 = 3650;

/// What the host tells the engine about itself.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct EngineConfig {
    /// The directory where the engine keeps its files (accounts, ledger, dedup state). Created if missing.
    pub data_directory: String,
    /// The platform the host runs on; decides which account kinds are offered.
    pub platform: Platform,
    /// Google's client; without it Google is listed as unconfigured.
    #[uniffi(default = None)]
    pub google_client: Option<GoogleClient>,
    /// Microsoft's client; without it Microsoft is listed as unconfigured.
    #[uniffi(default = None)]
    pub microsoft_client: Option<MicrosoftClient>,
    /// True when the host passes a `TextGenerator`.
    #[uniffi(default = false)]
    pub text_generation: bool,
    /// True when the host passes an `ExternalTokenSource` (enables the brokered Microsoft kind).
    #[uniffi(default = false)]
    pub external_tokens: bool,
    /// True when the host passes a `SystemCalendarSource`.
    #[uniffi(default = false)]
    pub system_calendar: bool,
    /// The current time zone identifier; the host reports changes with `setTimeZone` and `.system(.timeZoneChanged)`.
    pub time_zone: String,
    /// How many days back an iCalendar subscription keeps events.
    #[uniffi(default = 3)]
    pub ical_days_back: u32,
    /// How many days ahead an iCalendar subscription keeps events.
    #[uniffi(default = 7)]
    pub ical_days_ahead: u32,
}

/// A configuration that passed [`EngineConfig::validate`], in the engine's own types.
#[derive(Clone, Debug)]
pub struct ValidConfig {
    /// The data directory.
    pub data_directory: PathBuf,
    /// The platform flag.
    pub platform: calendar_core::connection::Platform,
    /// The time zone.
    pub zone: Zone,
    /// The connector settings.
    pub connectors: ConnectorSetupConfig,
}

fn invalid(reason: impl Into<String>) -> FfiError {
    FfiError::InvalidConfig {
        reason: reason.into(),
    }
}

impl EngineConfig {
    /// Checks the configuration; the error names the first thing wrong.
    pub fn validate(&self) -> Result<ValidConfig, FfiError> {
        if self.data_directory.trim().is_empty() {
            return Err(invalid("the data directory is empty"));
        }
        let zone = crate::custom_types::zone_from_identifier(&self.time_zone)?;
        if let Some(google) = &self.google_client
            && (google.id.is_empty() || google.secret.is_empty())
        {
            return Err(invalid("the Google client needs an id and a secret"));
        }
        if let Some(microsoft) = &self.microsoft_client
            && microsoft.id.is_empty()
        {
            return Err(invalid("the Microsoft client needs an id"));
        }
        for (name, days) in [
            ("back", self.ical_days_back),
            ("ahead", self.ical_days_ahead),
        ] {
            if days > MAX_RETENTION_DAYS {
                return Err(invalid(format!(
                    "iCalendar retention {name} is {days} days; the most is {MAX_RETENTION_DAYS}"
                )));
            }
        }
        Ok(ValidConfig {
            data_directory: PathBuf::from(&self.data_directory),
            platform: self.platform.to_flag(),
            zone,
            connectors: ConnectorSetupConfig {
                google: self
                    .google_client
                    .as_ref()
                    .map(|google| (google.id.clone(), google.secret.clone())),
                microsoft_client_id: self
                    .microsoft_client
                    .as_ref()
                    .map(|microsoft| microsoft.id.clone()),
                ical_days_back: i64::from(self.ical_days_back),
                ical_days_ahead: i64::from(self.ical_days_ahead),
            },
        })
    }
}
