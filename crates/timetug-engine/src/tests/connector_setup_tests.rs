//! Rust-only: `connector_setup::make` (moved from `timetug-engine-c`) registers the kinds the configuration allows and
//! lists the OAuth kinds that have no client.

use std::sync::Arc;

use calendar_core::connection::Platform;
use calendar_core::external_tokens::ExternalTokenSource;
use calendar_core::time::Zone;
use calendar_test_support::fake_external_token_source::FakeExternalTokenSource;
use calendar_test_support::fake_transport::FakeTransport;

use super::support::ManualClock;
use crate::connector_setup::{ConnectorSetupConfig, make};

fn ids(registry: &calendar_core::connection::ConnectorRegistry) -> Vec<String> {
    let mut ids: Vec<String> = registry
        .kinds_for(Platform::MACOS)
        .iter()
        .map(|kind| kind.id())
        .collect();
    ids.sort();
    ids
}

fn build(
    config: &ConnectorSetupConfig,
    tokens: bool,
) -> crate::connector_setup::ConnectorSetupResult {
    let token_source: Option<Arc<dyn ExternalTokenSource>> =
        tokens.then(|| Arc::new(FakeExternalTokenSource::new()) as Arc<dyn ExternalTokenSource>);
    make(
        config,
        Arc::new(FakeTransport::new()),
        Arc::new(ManualClock::new(jiff::Timestamp::UNIX_EPOCH)),
        Zone::utc(),
        token_source,
    )
}

#[test]
fn without_clients_only_the_password_and_link_kinds_exist() {
    let result = build(&ConnectorSetupConfig::default(), false);
    assert_eq!(ids(&result.registry), vec!["caldav", "icalsub", "icloud"]);
    assert_eq!(result.unconfigured_kind_ids, vec!["google", "microsoft"]);
}

#[test]
fn a_google_client_needs_both_its_id_and_its_secret() {
    for (google, configured) in [
        (Some(("id".to_string(), "secret".to_string())), true),
        (Some(("id".to_string(), String::new())), false),
        (Some((String::new(), "secret".to_string())), false),
        (None, false),
    ] {
        let config = ConnectorSetupConfig {
            google: google.clone(),
            ..ConnectorSetupConfig::default()
        };
        let result = build(&config, false);
        assert_eq!(
            result.registry.kind("google").is_some(),
            configured,
            "{google:?}"
        );
        assert_eq!(
            result.unconfigured_kind_ids.contains(&"google".to_string()),
            !configured,
            "{google:?}"
        );
    }
}

#[test]
fn the_brokered_microsoft_kind_needs_a_token_source_and_the_microsoft_client() {
    let with_client = ConnectorSetupConfig {
        microsoft_client_id: Some("ms".into()),
        ..ConnectorSetupConfig::default()
    };
    let browser_only = build(&with_client, false);
    assert!(browser_only.registry.kind("microsoft").is_some());
    assert!(
        browser_only
            .registry
            .kinds_for(Platform::MACOS)
            .iter()
            .all(|kind| kind.id() != "microsoft-brokered")
    );
    let both = build(&with_client, true);
    assert_eq!(
        both.registry.kinds_for(Platform::MACOS).len(),
        browser_only.registry.kinds_for(Platform::MACOS).len() + 1
    );
    let no_client = build(&ConnectorSetupConfig::default(), true);
    assert_eq!(
        no_client.unconfigured_kind_ids.len(),
        3,
        "{:?}",
        no_client.unconfigured_kind_ids
    );
}
