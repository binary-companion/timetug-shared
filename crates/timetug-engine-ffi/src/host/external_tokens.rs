//! `ExternalTokenSource`: access tokens from the operating system's account service instead of the browser flow.
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::external_tokens::{
    ExternalAccessToken as EngineToken, ExternalAccount as EngineAccount,
    ExternalSignInRequest as EngineRequest, ExternalSignInResult as EngineResult,
    ExternalTokenSource as EngineTokenSource,
};
use calendar_core::failure::Outcome;

use crate::errors::{HostError, HostService, failure_from_host_error};
use crate::values::host::{
    ExternalAccessToken, ExternalAccount, ExternalSignInRequest, ExternalSignInResult,
};
use crate::values::{ToEngine, ToMirror};

/// The host's account broker.
#[uniffi::export(foreign)]
#[async_trait]
pub trait ExternalTokenSource: Send + Sync {
    /// A short-lived access token for the account and scopes; `force_refresh` asks for a new one.
    async fn access_token(
        &self,
        account: ExternalAccount,
        scopes: Vec<String>,
        force_refresh: bool,
    ) -> Result<ExternalAccessToken, HostError>;
    /// Signs an account in (showing the broker's own UI) and returns the account and a token.
    async fn sign_in(
        &self,
        request: ExternalSignInRequest,
    ) -> Result<ExternalSignInResult, HostError>;
}

/// Presents the host's broker to the engine.
pub struct ExternalTokenSourceAdapter {
    host: Arc<dyn ExternalTokenSource>,
}

impl ExternalTokenSourceAdapter {
    /// Wraps the host's broker.
    pub fn new(host: Arc<dyn ExternalTokenSource>) -> ExternalTokenSourceAdapter {
        ExternalTokenSourceAdapter { host }
    }
}

fn token_failure<T>(result: Result<T, HostError>) -> Outcome<T> {
    result.map_err(|error| failure_from_host_error(error, HostService::ExternalToken))
}

#[async_trait]
impl EngineTokenSource for ExternalTokenSourceAdapter {
    async fn access_token(
        &self,
        account: &EngineAccount,
        scopes: &[String],
        force_refresh: bool,
    ) -> Outcome<EngineToken> {
        token_failure(
            self.host
                .access_token(account.clone().to_mirror(), scopes.to_vec(), force_refresh)
                .await,
        )
        .map(|token| token.to_engine())
    }

    async fn sign_in(&self, request: &EngineRequest) -> Outcome<EngineResult> {
        token_failure(self.host.sign_in(request.clone().to_mirror()).await)
            .map(|result| result.to_engine())
    }
}
