//! `AuthorizationInteraction` and `OAuthRedirectSession`: the browser sign-in and the credential form.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::connection::{
    AuthorizationInteraction as EngineInteraction, CredentialField as EngineField,
    OAuthRedirectSession as EngineSession,
};
use calendar_core::failure::Outcome;
use calendar_core::url_string::UrlString;

use crate::errors::{HostError, HostService, failure_from_host_error};
use crate::values::accounts::CredentialField;
use crate::values::{ToEngine, ToMirror};

/// One browser sign-in in progress: a local redirect address the browser comes back to.
#[uniffi::export(foreign)]
#[async_trait]
pub trait OAuthRedirectSession: Send + Sync {
    /// Where the provider sends the browser after the user signs in.
    fn redirect_uri(&self) -> UrlString;
    /// Opens the authorization page and waits for the browser to come back; returns the URL it came back with.
    async fn authorize(&self, authorization_url: UrlString) -> Result<UrlString, HostError>;
    /// Releases the redirect listener (not called `close`: the generated Kotlin class is AutoCloseable and `close` clashes). Called once, also when the sign-in was cancelled.
    async fn release(&self);
}

/// What `begin_oauth_redirect` returns: UniFFI cannot return a foreign trait object from an async foreign method directly
/// (docs/bindings-spike.md), so the session travels in a record.
#[derive(uniffi::Record)]
pub struct OAuthRedirectHandle {
    /// The session.
    pub session: Arc<dyn OAuthRedirectSession>,
}

/// The sign-in interactions the engine asks the host for.
#[uniffi::export(foreign)]
#[async_trait]
pub trait AuthorizationInteraction: Send + Sync {
    /// Starts listening for an OAuth redirect.
    async fn begin_oauth_redirect(&self) -> Result<OAuthRedirectHandle, HostError>;
    /// Shows a form for the fields and returns what the user typed, by field key.
    async fn prompt_credentials(
        &self,
        fields: Vec<CredentialField>,
    ) -> Result<HashMap<String, String>, HostError>;
}

fn general<T>(result: Result<T, HostError>) -> Outcome<T> {
    result.map_err(|error| failure_from_host_error(error, HostService::General))
}

struct SessionAdapter {
    host: Arc<dyn OAuthRedirectSession>,
}

#[async_trait]
impl EngineSession for SessionAdapter {
    fn redirect_uri(&self) -> UrlString {
        self.host.redirect_uri()
    }

    async fn authorize(&self, authorization_url: &UrlString) -> Outcome<UrlString> {
        general(self.host.authorize(authorization_url.clone()).await)
    }

    async fn close(&self) {
        self.host.release().await;
    }
}

/// Presents the host's sign-in interactions to the engine.
pub struct AuthorizationInteractionAdapter {
    host: Arc<dyn AuthorizationInteraction>,
}

impl AuthorizationInteractionAdapter {
    /// Wraps the host's interactions.
    pub fn new(host: Arc<dyn AuthorizationInteraction>) -> AuthorizationInteractionAdapter {
        AuthorizationInteractionAdapter { host }
    }
}

#[async_trait]
impl EngineInteraction for AuthorizationInteractionAdapter {
    async fn begin_oauth_redirect(&self) -> Outcome<Box<dyn EngineSession>> {
        let handle = general(self.host.begin_oauth_redirect().await)?;
        Ok(Box::new(SessionAdapter {
            host: handle.session,
        }))
    }

    async fn prompt_credentials(
        &self,
        fields: &[EngineField],
    ) -> Outcome<BTreeMap<String, String>> {
        let fields: Vec<CredentialField> = fields.to_vec().to_mirror();
        general(self.host.prompt_credentials(fields).await).map(|values| values.to_engine())
    }
}
