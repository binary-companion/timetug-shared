//! Port of `Connectors.swift`: the C interface's front to `timetug_engine::connector_setup`, which owns the registration
//! shared with the bindings.

use std::sync::Arc;

use calendar_core::external_tokens::ExternalTokenSource;
use calendar_core::http::HttpTransport;
use calendar_core::time::Zone;
use timetug_engine::clock::EngineClock;
use timetug_engine::connector_setup::{self, ConnectorSetupConfig};
use timetug_engine_protocol::config::WireConfig;

pub use timetug_engine::connector_setup::ConnectorSetupResult;

/// The connector kinds an engine behind the C interface offers (the C interface keeps the Mac app's retention of 3 days
/// back and 7 ahead).
pub enum ConnectorSetup {}

impl ConnectorSetup {
    /// Builds the registry of network connector kinds from the wire configuration; see
    /// [`timetug_engine::connector_setup::make`].
    pub fn make(
        config: &WireConfig,
        transport: Arc<dyn HttpTransport>,
        clock: Arc<dyn EngineClock>,
        default_zone: Zone,
        token_source: Option<Arc<dyn ExternalTokenSource>>,
    ) -> ConnectorSetupResult {
        let google = config.connectors.get("google").and_then(|google| {
            let secret = google.client_secret.as_deref().filter(|s| !s.is_empty())?;
            (!google.client_id.is_empty()).then(|| (google.client_id.clone(), secret.to_string()))
        });
        let microsoft_client_id = config
            .connectors
            .get("microsoft")
            .filter(|microsoft| !microsoft.client_id.is_empty())
            .map(|microsoft| microsoft.client_id.clone());
        let setup = ConnectorSetupConfig {
            google,
            microsoft_client_id,
            ..ConnectorSetupConfig::default()
        };
        connector_setup::make(&setup, transport, clock, default_zone, token_source)
    }
}
