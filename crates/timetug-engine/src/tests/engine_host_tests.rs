//! Rust-only: `EngineHost` (a struct with no Swift test of its own) builds from the seams with the Swift defaults, and
//! the default zone is a zone the table knows.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::{
    AuthorizationInteraction, ConnectorRegistry, CredentialField, CredentialHelp,
    InMemoryCredentialStore, OAuthRedirectSession,
};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::time::Zone;

use super::support::TempDir;
use crate::host::{EngineHost, SignInPrompting, current_zone};
use crate::settings::InMemorySettingsStorage;

struct NoInteraction;

#[async_trait]
impl AuthorizationInteraction for NoInteraction {
    async fn begin_oauth_redirect(&self) -> Outcome<Box<dyn OAuthRedirectSession>> {
        Err(Failure::Cancelled)
    }

    async fn prompt_credentials(
        &self,
        _fields: &[CredentialField],
    ) -> Outcome<BTreeMap<String, String>> {
        Err(Failure::Cancelled)
    }
}

struct NoSignIn;

#[async_trait]
impl SignInPrompting for NoSignIn {
    async fn prepare(
        &self,
        _title: String,
        _help: Option<CredentialHelp>,
        _values: BTreeMap<String, String>,
        _error: Option<String>,
    ) {
    }

    async fn last_non_secret_values(&self) -> Option<BTreeMap<String, String>> {
        None
    }
}

#[test]
fn a_host_builds_with_the_swift_defaults() {
    let dir = TempDir::make();
    let host = EngineHost::new(
        dir.path(),
        Arc::new(InMemorySettingsStorage::default()),
        Arc::new(InMemoryCredentialStore::new()),
        Arc::new(NoInteraction),
        Arc::new(NoSignIn),
        ConnectorRegistry::new(),
    );
    assert_eq!(host.data_directory, dir.path());
    assert!(host.unconfigured_kind_ids.is_empty());
    assert!(host.build_system_source.is_none() && host.request_system_access.is_none());
    assert!(host.text_generator.is_none());
    // The default sink drops diagnostics; the default clock is the wall clock.
    (host.diagnostics)(crate::diagnostic::EngineDiagnostic::new(
        crate::diagnostic::Level::Info,
        "t",
        "m",
    ));
    let now = host.clock.now();
    assert!(now > jiff::Timestamp::from_second(1_700_000_000).unwrap());
    let zone = (host.zone)();
    assert_eq!(Zone::named(zone.identifier()), Some(zone));
}

#[test]
fn the_current_zone_is_a_known_zone() {
    let zone = current_zone();
    assert_eq!(Zone::named(zone.identifier()), Some(zone));
}
