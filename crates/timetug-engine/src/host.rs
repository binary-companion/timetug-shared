//! Port of `EngineHost.swift`.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::{
    AuthorizationInteraction, ConnectorRegistry, CredentialHelp, CredentialStore,
};
use calendar_core::time::Zone;

use crate::clock::{EngineClock, SystemClock};
use crate::diagnostic::{EngineDiagnosticSink, ignore_diagnostics};
use crate::inference::text_generator::TextGenerator;
use crate::settings::SettingsStorage;

/// The engine's view of the host's sign-in form (the Mac `CredentialPrompter`; Windows implements the same).
/// `prepare` arms the form for the next `prompt_credentials` call; `last_non_secret_values` is what the user last
/// typed, without secrets, or `None` when the sign-in failed before a form was submitted.
#[async_trait]
pub trait SignInPrompting: Send + Sync {
    async fn prepare(
        &self,
        title: String,
        help: Option<CredentialHelp>,
        values: BTreeMap<String, String>,
        error: Option<String>,
    );
    async fn last_non_secret_values(&self) -> Option<BTreeMap<String, String>>;
}

/// Swift's `@Sendable () -> any TimeTugCore.CalendarSource`.
pub type BuildSystemSource = Arc<dyn Fn() -> Arc<dyn timetug_core::CalendarSource> + Send + Sync>;

/// Swift's `@Sendable () async -> Void`.
pub type RequestSystemAccess =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Swift's `calendar: @Sendable () -> Calendar`: the engine asks for the current zone wherever it needs the calendar.
pub type ZoneProvider = Arc<dyn Fn() -> Zone + Send + Sync>;

/// Everything the host supplies to the engine.
#[derive(Clone)]
pub struct EngineHost {
    pub data_directory: PathBuf,
    pub settings: Arc<dyn SettingsStorage>,
    pub credentials: Arc<dyn CredentialStore>,
    pub interaction: Arc<dyn AuthorizationInteraction>,
    pub sign_in: Arc<dyn SignInPrompting>,
    pub registry: ConnectorRegistry,
    /// Kind ids supported in code but not registered in this build (for example Google without an OAuth client).
    pub unconfigured_kind_ids: Vec<String>,
    pub build_system_source: Option<BuildSystemSource>,
    pub request_system_access: Option<RequestSystemAccess>,
    pub text_generator: Option<Arc<dyn TextGenerator>>,
    pub diagnostics: EngineDiagnosticSink,
    pub clock: Arc<dyn EngineClock>,
    pub zone: ZoneProvider,
}

impl EngineHost {
    /// Swift's `init` with every optional argument at its default: no unconfigured kinds, no system source, no system
    /// access request, no text generator, diagnostics dropped, `SystemClock`, and the machine's current zone.
    /// Assign the public fields for the rest.
    pub fn new(
        data_directory: impl Into<PathBuf>,
        settings: Arc<dyn SettingsStorage>,
        credentials: Arc<dyn CredentialStore>,
        interaction: Arc<dyn AuthorizationInteraction>,
        sign_in: Arc<dyn SignInPrompting>,
        registry: ConnectorRegistry,
    ) -> EngineHost {
        EngineHost {
            data_directory: data_directory.into(),
            settings,
            credentials,
            interaction,
            sign_in,
            registry,
            unconfigured_kind_ids: Vec::new(),
            build_system_source: None,
            request_system_access: None,
            text_generator: None,
            diagnostics: ignore_diagnostics(),
            clock: Arc::new(SystemClock),
            zone: Arc::new(current_zone),
        }
    }
}

/// `Calendar.current.timeZone`: the machine's zone as `jiff` reports it, or UTC when the machine names none that
/// the zone table knows.
pub fn current_zone() -> Zone {
    jiff::tz::TimeZone::system()
        .iana_name()
        .and_then(Zone::named)
        .unwrap_or_else(Zone::utc)
}
