//! Port of `Connectors.swift`.

use std::sync::Arc;

use calendar_caldav::{CalDavConnectorKind, ICloudConnectorKind};
use calendar_core::connection::ConnectorRegistry;
use calendar_core::external_tokens::ExternalTokenSource;
use calendar_core::http::HttpTransport;
use calendar_core::time::{Now, Sleeper, Zone, add_seconds};
use calendar_google::{GoogleConnectorKind, GoogleOAuthConfig};
use calendar_ical_subscription::{ICalSubscriptionKind, RetentionWindow};
use calendar_microsoft::{
    MicrosoftBrokeredConnectorKind, MicrosoftConnectorKind, MicrosoftOAuthConfig,
};
use timetug_engine::clock::EngineClock;
use timetug_engine::host::current_zone;
use timetug_engine_protocol::config::WireConfig;

/// The connector kinds an engine behind the C interface offers: the library's network kinds, as the Mac app registers
/// them (`AppConnectors`), minus EventKit. Every kind sends HTTP through the host and reads time and sleeps through the
/// engine clock, so a `testing` engine's polls and token expiry follow `test.advanceClock` too.
pub enum ConnectorSetup {}

/// What [`ConnectorSetup::make`] builds.
pub struct ConnectorSetupResult {
    /// The registry of connector kinds, ready to hand to the engine.
    pub registry: ConnectorRegistry,
    /// OAuth kinds this host build has no client for; listed as unconfigured in `availableAccountKinds`.
    pub unconfigured_kind_ids: Vec<String>,
}

impl ConnectorSetup {
    /// Builds the registry of network connector kinds. HTTP goes through `transport`; time and sleeping go through
    /// `clock`; `default_zone` is the zone iCloud and CalDAV kinds default to; `token_source`, when present, supplies
    /// externally managed OAuth tokens. Kinds without a client in `config` are returned in `unconfigured_kind_ids`.
    pub fn make(
        config: &WireConfig,
        transport: Arc<dyn HttpTransport>,
        clock: Arc<dyn EngineClock>,
        default_zone: Zone,
        token_source: Option<Arc<dyn ExternalTokenSource>>,
    ) -> ConnectorSetupResult {
        let now_clock = clock.clone();
        let now: Now = Arc::new(move || now_clock.now());
        let sleep_clock = clock;
        let sleep: Sleeper = Arc::new(move |seconds| {
            let clock = sleep_clock.clone();
            Box::pin(async move {
                let deadline = add_seconds(clock.now(), seconds);
                clock.sleep_until(deadline).await
            })
        });
        let mut registry = ConnectorRegistry::new();
        let mut unconfigured = Vec::new();
        // Swift's iCloud and CalDAV kinds default their zone to `TimeZone.current`: the process zone.
        registry.register(Arc::new(ICloudConnectorKind::new(
            transport.clone(),
            now.clone(),
            sleep.clone(),
            current_zone(),
        )));
        registry.register(Arc::new(CalDavConnectorKind::new(
            transport.clone(),
            now.clone(),
            sleep.clone(),
            current_zone(),
        )));
        registry.register(Arc::new(
            ICalSubscriptionKind::new(transport.clone(), now.clone(), sleep.clone(), default_zone)
                .with_retention(RetentionWindow::new(3, 7)),
        ));
        // Google's desktop client has a secret as well as an id; both are needed for the token exchange.
        let google = config.connectors.get("google").and_then(|google| {
            let secret = google.client_secret.as_deref().filter(|s| !s.is_empty())?;
            (!google.client_id.is_empty()).then(|| (google.client_id.clone(), secret.to_string()))
        });
        match google {
            Some((client_id, secret)) => registry.register(Arc::new(GoogleConnectorKind::new(
                GoogleOAuthConfig::new(client_id, secret),
                transport.clone(),
                now.clone(),
                sleep.clone(),
            ))),
            None => unconfigured.push(GoogleConnectorKind::KIND_ID.to_string()),
        }
        let microsoft_client = config
            .connectors
            .get("microsoft")
            .filter(|microsoft| !microsoft.client_id.is_empty())
            .map(|microsoft| microsoft.client_id.clone());
        match &microsoft_client {
            Some(client_id) => registry.register(Arc::new(MicrosoftConnectorKind::new(
                MicrosoftOAuthConfig::new(client_id.clone()),
                transport.clone(),
                now.clone(),
                sleep.clone(),
            ))),
            None => unconfigured.push(MicrosoftConnectorKind::KIND_ID.to_string()),
        }
        // The brokered kind exists only for a host that opted in (`token_source` is `Some` exactly then), and it shares
        // the browser kind's Microsoft client id: there is no separate configuration entry.
        if let Some(token_source) = token_source {
            if microsoft_client.is_some() {
                registry.register(Arc::new(MicrosoftBrokeredConnectorKind::new(
                    token_source,
                    transport,
                    now,
                    sleep,
                )));
            } else {
                unconfigured.push(MicrosoftBrokeredConnectorKind::KIND_ID.to_string());
            }
        }
        ConnectorSetupResult {
            registry,
            unconfigured_kind_ids: unconfigured,
        }
    }
}
