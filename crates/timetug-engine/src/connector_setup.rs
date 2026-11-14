//! The connector kinds an engine offers, built from plain configuration. Both front doors (the C interface and the
//! bindings) call [`make`], so they register the same kinds the same way. Moved here from `timetug-engine-c`
//! (port of `Connectors.swift`).

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

use crate::clock::EngineClock;
use crate::host::current_zone;

/// What decides which connector kinds exist and how they behave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorSetupConfig {
    /// Google's desktop client: the id and the secret (both are needed for the token exchange); `None` or an empty part
    /// leaves Google unconfigured.
    pub google: Option<(String, String)>,
    /// Microsoft's client id; `None` or empty leaves Microsoft (and the brokered kind) unconfigured.
    pub microsoft_client_id: Option<String>,
    /// How many days back an iCalendar subscription keeps events.
    pub ical_days_back: i64,
    /// How many days ahead an iCalendar subscription keeps events.
    pub ical_days_ahead: i64,
}

impl Default for ConnectorSetupConfig {
    /// No clients, and the retention the Mac app uses (3 days back, 7 ahead).
    fn default() -> ConnectorSetupConfig {
        ConnectorSetupConfig {
            google: None,
            microsoft_client_id: None,
            ical_days_back: 3,
            ical_days_ahead: 7,
        }
    }
}

/// What [`make`] builds.
pub struct ConnectorSetupResult {
    /// The registry of connector kinds, ready to hand to the engine.
    pub registry: ConnectorRegistry,
    /// OAuth kinds this host build has no client for; listed as unconfigured in `availableAccountKinds`.
    pub unconfigured_kind_ids: Vec<String>,
}

/// Builds the registry of network connector kinds, as the Mac app registers them (`AppConnectors`) minus EventKit. HTTP
/// goes through `transport`; time and sleeping go through `clock`; `default_zone` is the zone iCalendar subscriptions
/// default to; `token_source`, when present, supplies externally managed OAuth tokens. Kinds without a client in `config`
/// are returned in `unconfigured_kind_ids`.
pub fn make(
    config: &ConnectorSetupConfig,
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
            .with_retention(RetentionWindow::new(
                config.ical_days_back,
                config.ical_days_ahead,
            )),
    ));
    // Google's desktop client has a secret as well as an id; both are needed for the token exchange.
    let google = config.google.as_ref().and_then(|(client_id, secret)| {
        (!client_id.is_empty() && !secret.is_empty()).then(|| (client_id.clone(), secret.clone()))
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
        .microsoft_client_id
        .as_ref()
        .filter(|client_id| !client_id.is_empty())
        .cloned();
    match &microsoft_client {
        Some(client_id) => registry.register(Arc::new(MicrosoftConnectorKind::new(
            MicrosoftOAuthConfig::new(client_id.clone()),
            transport.clone(),
            now.clone(),
            sleep.clone(),
        ))),
        None => unconfigured.push(MicrosoftConnectorKind::KIND_ID.to_string()),
    }
    // The brokered kind exists only for a host that opted in (`token_source` is `Some` exactly then), and it shares the
    // browser kind's Microsoft client id: there is no separate configuration entry.
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
