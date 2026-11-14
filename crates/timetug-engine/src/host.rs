//! Port of `EngineHost.swift`.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::{
    AuthorizationInteraction, ConnectorRegistry, CredentialHelp, CredentialStore, Platform,
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
    /// Arms the form for the next sign-in prompt with its `title`, optional `help`, prefilled `values` and the
    /// previous
    /// attempt's `error`.
    async fn prepare(
        &self,
        title: String,
        help: Option<CredentialHelp>,
        values: BTreeMap<String, String>,
        error: Option<String>,
    );
    /// The values the user typed in the last submitted form, without secrets; `None` when sign-in failed before a
    /// form was
    /// submitted.
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
    /// Directory where the engine keeps its files (accounts, ledger, dedup state, and `settings.json` for the C
    /// interface).
    pub data_directory: PathBuf,
    /// Where preferences are stored.
    pub settings: Arc<dyn SettingsStorage>,
    /// Where secrets such as tokens and passwords are stored.
    pub credentials: Arc<dyn CredentialStore>,
    /// How the engine opens a browser and waits for an OAuth sign-in.
    pub interaction: Arc<dyn AuthorizationInteraction>,
    /// The host's sign-in form for password kinds.
    pub sign_in: Arc<dyn SignInPrompting>,
    /// The connector kinds this build can sign in with.
    pub registry: ConnectorRegistry,
    /// Kind ids supported in code but not registered in this build (for example Google without an OAuth client).
    pub unconfigured_kind_ids: Vec<String>,
    /// Builds the system calendar source (EventKit on the Mac); `None` when the platform has none.
    pub build_system_source: Option<BuildSystemSource>,
    /// Asks the user for system calendar access; `None` when there is nothing to ask.
    pub request_system_access: Option<RequestSystemAccess>,
    /// The host's text model for judging duplicates; `None` disables model inference.
    pub text_generator: Option<Arc<dyn TextGenerator>>,
    /// Where the engine sends its diagnostics.
    pub diagnostics: EngineDiagnosticSink,
    /// The only time source the engine reads; `SystemClock` unless a test supplies another.
    pub clock: Arc<dyn EngineClock>,
    /// Returns the current time zone whenever the engine needs the calendar.
    pub zone: ZoneProvider,
    /// The platform the host runs on, which decides the account kinds offered: `Platform::current()` unless the host
    /// says otherwise (the bindings pass it in their configuration; a front end for a platform without a compile-time
    /// target of its own, such as Android, needs it).
    pub platform: Platform,
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
            platform: Platform::current(),
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
