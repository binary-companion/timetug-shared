//! `CredentialStore`: where the host keeps secrets (the Keychain on Apple platforms).
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::{
    CredentialSnapshot as EngineSnapshot, CredentialStore as EngineCredentialStore,
};
use calendar_core::failure::Outcome;
use uuid::Uuid;

use crate::errors::{HostError, HostService, failure_from_host_error};
use crate::values::accounts::CredentialSnapshot;
use crate::values::{ToEngine, ToMirror};

/// Secrets by connection id.
#[uniffi::export(foreign)]
#[async_trait]
pub trait CredentialStore: Send + Sync {
    /// The stored secrets of a connection, or `None`.
    async fn secrets(
        &self,
        connection_id: String,
    ) -> Result<Option<HashMap<String, String>>, HostError>;
    /// Replaces the secrets of a connection.
    async fn set_secrets(
        &self,
        secrets: HashMap<String, String>,
        connection_id: String,
    ) -> Result<(), HostError>;
    /// Removes the secrets of a connection.
    async fn remove_secrets(&self, connection_id: String) -> Result<(), HostError>;
    /// The secrets and their revision, for a compare-and-set.
    async fn credential_snapshot(
        &self,
        connection_id: String,
    ) -> Result<Option<CredentialSnapshot>, HostError>;
    /// Stores a new refresh token when the stored revision is still `expected_revision`; false when it is not.
    async fn update_refresh_token(
        &self,
        token: String,
        connection_id: String,
        expected_revision: Uuid,
    ) -> Result<bool, HostError>;
}

/// Presents the host's credential store to the engine.
pub struct CredentialStoreAdapter {
    host: Arc<dyn CredentialStore>,
}

impl CredentialStoreAdapter {
    /// Wraps the host's store.
    pub fn new(host: Arc<dyn CredentialStore>) -> CredentialStoreAdapter {
        CredentialStoreAdapter { host }
    }
}

fn general<T>(result: Result<T, HostError>) -> Outcome<T> {
    result.map_err(|error| failure_from_host_error(error, HostService::General))
}

#[async_trait]
impl EngineCredentialStore for CredentialStoreAdapter {
    async fn secrets(&self, connection_id: &str) -> Outcome<Option<BTreeMap<String, String>>> {
        general(self.host.secrets(connection_id.to_string()).await)
            .map(|secrets| secrets.map(|s| s.to_engine()))
    }

    async fn set_secrets(
        &self,
        secrets: BTreeMap<String, String>,
        connection_id: &str,
    ) -> Outcome<()> {
        general(
            self.host
                .set_secrets(secrets.to_mirror(), connection_id.to_string())
                .await,
        )
    }

    async fn remove_secrets(&self, connection_id: &str) -> Outcome<()> {
        general(self.host.remove_secrets(connection_id.to_string()).await)
    }

    async fn credential_snapshot(&self, connection_id: &str) -> Outcome<Option<EngineSnapshot>> {
        general(
            self.host
                .credential_snapshot(connection_id.to_string())
                .await,
        )
        .map(|snapshot| snapshot.map(|s| s.to_engine()))
    }

    async fn update_refresh_token(
        &self,
        token: &str,
        connection_id: &str,
        expected_revision: Uuid,
    ) -> Outcome<bool> {
        general(
            self.host
                .update_refresh_token(
                    token.to_string(),
                    connection_id.to_string(),
                    expected_revision,
                )
                .await,
        )
    }
}
