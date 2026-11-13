//! Port of `RemoteHostServices.swift`: the engine's host seams implemented as `service.*` requests answered through
//! `tt_engine_reply`.
//!
//! Timeouts run on the engine clock. HTTP requests time out after 60 seconds (the spec's rule) and so do the other
//! requests no person takes part in (credentials, closing a redirect, preparing a form, text generation), so a host
//! that never answers cannot stall a refresh forever. The steps a person takes part in (the browser sign-in and the
//! credential form) have no timeout; `account.cancel` and destroy end them.
//!
//! # Cancellation
//!
//! Swift's `withTaskCancellationHandler` has no Rust counterpart: a cancelled task is a dropped future. Where Swift
//! reacted to the cancellation (closing a redirect session, telling the host to close its sign-in dialog), a guard
//! whose `Drop` sends the same best-effort request does the work (conventions section 10, item 11). Such a request is
//! started with `Handle::try_current`, so it needs the engine's runtime and is skipped when none is current; after
//! destroy the table is closed, and the request fails at once without being sent.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use base64::Engine as _;
use base64::alphabet;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use calendar_core::connection::{
    AuthorizationInteraction, CredentialField, CredentialHelp, CredentialSnapshot, CredentialStore,
    OAuthRedirectSession,
};
use calendar_core::external_tokens::{
    ExternalAccessToken, ExternalAccount, ExternalSignInRequest, ExternalSignInResult,
    ExternalTokenSource,
};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::http::{HttpRequest, HttpResponse, HttpTransport};
use calendar_core::source_types::SourceError;
use calendar_core::url_string::UrlString;
use serde::Serialize;
use serde::de::IgnoredAny;
use serde_json::Value;
use timetug_core::dedup::adjudication::EngineInfo;
use timetug_engine::diagnostic::{EngineDiagnostic, EngineDiagnosticSink, Level};
use timetug_engine::host::SignInPrompting;
use timetug_engine::inference::text_generator::{TextGenerator, TextGeneratorAvailability};
use timetug_engine_protocol::service_messages::{
    AuthAuthorizeReply, AuthAuthorizeRequest, AuthBeginRedirectReply, AuthCloseRedirectRequest,
    AuthPromptCredentialsReply, AuthPromptCredentialsRequest, CredentialsGetReply,
    CredentialsKeyRequest, CredentialsSetRequest, CredentialsSnapshotReply,
    CredentialsUpdateRefreshTokenReply, CredentialsUpdateRefreshTokenRequest, HttpSendReply,
    HttpSendRequest, ReplyErrorKind, SignInLastValuesReply, SignInPrepareRequest,
    TextAvailabilityReply, TextGenerateReply, TextGenerateRequest, TokenCancelSignInRequest,
    TokenGetReply, TokenGetRequest, TokenSignInReply, TokenSignInRequest, WireCredentialField,
    WireCredentialHelp,
};
use timetug_engine_protocol::state_mapping::json_value;
use timetug_engine_protocol::wire_types::WireDates;
use uuid::Uuid;

use crate::request_table::{RequestError, RequestTable, ServiceError};

/// Seconds the engine waits for a host answer that involves no person.
pub const HOST_SERVICE_TIMEOUT: f64 = 60.0;

/// Foundation's `Data(base64Encoded:)` takes the standard alphabet; this decoder also takes a missing `=` padding.
const BASE64_LENIENT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

fn wire<T: Serialize>(value: &T) -> Value {
    json_value(value)
}

fn failed(message: &str) -> RequestError {
    RequestError::Service(ServiceError {
        kind: ReplyErrorKind::Failed,
        message: message.to_string(),
    })
}

/// Swift's `uuidString`: upper case.
fn uuid_string(uuid: Uuid) -> String {
    uuid.hyphenated().to_string().to_uppercase()
}

/// Runs a best-effort request on the engine's runtime without waiting for it; skipped when no runtime is current.
fn spawn_detached(future: impl Future<Output = ()> + Send + 'static) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        drop(handle.spawn(future));
    }
}

// MARK: - Credentials

/// The host's credential store, over `service.credentials.*`.
pub struct RemoteCredentialStore {
    table: Arc<RequestTable>,
    /// The host's own spelling of each connection's latest revision. The engine holds revisions as `Uuid`s, whose
    /// `uuidString` is upper case; a host that wrote the revision in lower case (.NET's `Guid.ToString()`) must get back
    /// exactly the string it sent, or its compare-and-set never matches.
    spellings: RevisionSpellings,
}

impl RemoteCredentialStore {
    /// Creates the store, sending its requests through `table`.
    pub fn new(table: Arc<RequestTable>) -> RemoteCredentialStore {
        RemoteCredentialStore {
            table,
            spellings: RevisionSpellings::default(),
        }
    }

    fn key(connection_id: &str) -> Value {
        wire(&CredentialsKeyRequest {
            connection_id: connection_id.to_string(),
        })
    }
}

#[async_trait]
impl CredentialStore for RemoteCredentialStore {
    async fn secrets(&self, connection_id: &str) -> Outcome<Option<BTreeMap<String, String>>> {
        let reply: CredentialsGetReply = self
            .table
            .call(
                "service.credentials.get",
                Some(Self::key(connection_id)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await?;
        Ok(reply.secrets)
    }

    async fn set_secrets(
        &self,
        secrets: BTreeMap<String, String>,
        connection_id: &str,
    ) -> Outcome<()> {
        let request = CredentialsSetRequest {
            connection_id: connection_id.to_string(),
            secrets,
        };
        self.table
            .call::<IgnoredAny>(
                "service.credentials.set",
                Some(wire(&request)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await?;
        Ok(())
    }

    async fn remove_secrets(&self, connection_id: &str) -> Outcome<()> {
        self.table
            .call::<IgnoredAny>(
                "service.credentials.remove",
                Some(Self::key(connection_id)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await?;
        Ok(())
    }

    async fn credential_snapshot(
        &self,
        connection_id: &str,
    ) -> Outcome<Option<CredentialSnapshot>> {
        let reply: CredentialsSnapshotReply = self
            .table
            .call(
                "service.credentials.snapshot",
                Some(Self::key(connection_id)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await?;
        let Some(snapshot) = reply.snapshot else {
            return Ok(None);
        };
        let Some(revision) = parse_uuid(&snapshot.revision) else {
            return Err(failed("the credential revision is not a UUID").into());
        };
        self.spellings
            .record(&snapshot.revision, revision, connection_id);
        Ok(Some(CredentialSnapshot::new(snapshot.secrets, revision)))
    }

    async fn update_refresh_token(
        &self,
        token: &str,
        connection_id: &str,
        expected_revision: Uuid,
    ) -> Outcome<bool> {
        let request = CredentialsUpdateRefreshTokenRequest {
            connection_id: connection_id.to_string(),
            token: token.to_string(),
            expected_revision: self.spellings.spelling(expected_revision, connection_id),
        };
        let reply: CredentialsUpdateRefreshTokenReply = self
            .table
            .call(
                "service.credentials.updateRefreshToken",
                Some(wire(&request)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await?;
        Ok(reply.updated)
    }
}

/// `UUID(uuidString:)`: the 36-character hyphenated form, in either case.
fn parse_uuid(text: &str) -> Option<Uuid> {
    if text.len() != 36 {
        return None;
    }
    Uuid::parse_str(text).ok()
}

/// The last revision string the host gave per connection (one entry per connection, so it stays small).
#[derive(Default)]
pub struct RevisionSpellings {
    latest: Mutex<BTreeMap<String, (Uuid, String)>>,
}

impl RevisionSpellings {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, (Uuid, String)>> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Remembers the host's spelling `text` of the latest `revision` for `connection_id`.
    pub fn record(&self, text: &str, revision: Uuid, connection_id: &str) {
        self.lock()
            .insert(connection_id.to_string(), (revision, text.to_string()));
    }

    /// The host's spelling when `revision` is the one it gave last for the connection, otherwise the upper-case form.
    pub fn spelling(&self, revision: Uuid, connection_id: &str) -> String {
        match self.lock().get(connection_id) {
            Some((latest, text)) if *latest == revision => text.clone(),
            _ => uuid_string(revision),
        }
    }
}

// MARK: - HTTP

/// HTTP through the host. Any status is a result; a host transport failure, any other error reply and the 60 s timeout
/// are `SourceError::Network`; a cancelled reply is a cancellation.
pub struct RemoteHttpTransport {
    table: Arc<RequestTable>,
}

impl RemoteHttpTransport {
    /// The time in seconds after which a request that has no answer fails.
    pub const TIMEOUT: f64 = 60.0;

    /// Creates the transport, sending its requests through `table`.
    pub fn new(table: Arc<RequestTable>) -> RemoteHttpTransport {
        RemoteHttpTransport { table }
    }
}

#[async_trait]
impl HttpTransport for RemoteHttpTransport {
    async fn send(&self, request: HttpRequest) -> Outcome<HttpResponse> {
        let outgoing = HttpSendRequest {
            method: request.method,
            url: request.url.as_str().to_string(),
            headers: request.headers,
            body: request.body.map(|body| BASE64_LENIENT.encode(body)),
        };
        let reply: HttpSendReply = match self
            .table
            .call(
                "service.http.send",
                Some(wire(&outgoing)),
                Some(Self::TIMEOUT),
            )
            .await
        {
            Ok(reply) => reply,
            Err(RequestError::Service(error)) => {
                return Err(Failure::Source(SourceError::Network(error.to_string())));
            }
            Err(RequestError::Cancelled) => return Err(Failure::Cancelled),
        };
        let body = match reply.body {
            Some(encoded) => BASE64_LENIENT.decode(encoded).map_err(|_| {
                Failure::Source(SourceError::InvalidResponse(
                    "the host's HTTP body is not base64".to_string(),
                ))
            })?,
            None => Vec::new(),
        };
        Ok(HttpResponse::new(
            reply.status,
            reply.headers.unwrap_or_default(),
            body,
        ))
    }
}

// MARK: - Sign-in

/// The host's sign-in interaction (OAuth redirect and related steps), over `service.auth.*`.
pub struct RemoteAuthorizationInteraction {
    table: Arc<RequestTable>,
}

impl RemoteAuthorizationInteraction {
    /// Creates the interaction, sending its requests through `table`.
    pub fn new(table: Arc<RequestTable>) -> RemoteAuthorizationInteraction {
        RemoteAuthorizationInteraction { table }
    }
}

#[async_trait]
impl AuthorizationInteraction for RemoteAuthorizationInteraction {
    async fn begin_oauth_redirect(&self) -> Outcome<Box<dyn OAuthRedirectSession>> {
        let reply: AuthBeginRedirectReply = self
            .table
            .call("service.auth.beginRedirect", None, None)
            .await?;
        let Some(redirect_uri) = UrlString::parse(&reply.redirect_uri) else {
            return Err(failed("the redirect URI is not a URL").into());
        };
        Ok(Box::new(RemoteRedirectSession {
            table: self.table.clone(),
            session_id: reply.session_id,
            redirect_uri,
            closed: AtomicBool::new(false),
        }))
    }

    async fn prompt_credentials(
        &self,
        fields: &[CredentialField],
    ) -> Outcome<BTreeMap<String, String>> {
        let request = AuthPromptCredentialsRequest {
            fields: fields
                .iter()
                .map(|field| WireCredentialField {
                    key: field.key.clone(),
                    label: field.label.clone(),
                    is_secret: field.is_secret,
                })
                .collect(),
        };
        let reply: AuthPromptCredentialsReply = self
            .table
            .call("service.auth.promptCredentials", Some(wire(&request)), None)
            .await?;
        Ok(reply.values)
    }
}

/// One redirect session at the host. `close` tells the host to end it; the connector kinds of the library call it only
/// after the flow returns, not when the flow is cancelled, so a session dropped without `close` (a cancelled sign-in)
/// closes itself here, as Swift's cancelled flow did through its `defer`.
pub struct RemoteRedirectSession {
    table: Arc<RequestTable>,
    session_id: String,
    redirect_uri: UrlString,
    closed: AtomicBool,
}

impl RemoteRedirectSession {
    /// Best effort: the sign-in already has its answer, so a failure to close is not reported.
    async fn send_close(table: &RequestTable, session_id: &str) {
        let request = AuthCloseRedirectRequest {
            session_id: session_id.to_string(),
        };
        let _ = table
            .call::<IgnoredAny>(
                "service.auth.closeRedirect",
                Some(wire(&request)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await;
    }
}

#[async_trait]
impl OAuthRedirectSession for RemoteRedirectSession {
    fn redirect_uri(&self) -> UrlString {
        self.redirect_uri.clone()
    }

    async fn authorize(&self, authorization_url: &UrlString) -> Outcome<UrlString> {
        let request = AuthAuthorizeRequest {
            session_id: self.session_id.clone(),
            url: authorization_url.as_str().to_string(),
        };
        let reply: AuthAuthorizeReply = self
            .table
            .call("service.auth.authorize", Some(wire(&request)), None)
            .await?;
        UrlString::parse(&reply.url).ok_or_else(|| failed("the redirect is not a URL").into())
    }

    async fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        Self::send_close(&self.table, &self.session_id).await;
    }
}

impl Drop for RemoteRedirectSession {
    fn drop(&mut self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let table = self.table.clone();
        let session_id = std::mem::take(&mut self.session_id);
        spawn_detached(async move { Self::send_close(&table, &session_id).await });
    }
}

// MARK: - The sign-in form

/// The host's credential sign-in form.
pub struct RemoteSignInPrompting {
    table: Arc<RequestTable>,
}

impl RemoteSignInPrompting {
    /// Creates the prompting, sending its requests through `table`.
    pub fn new(table: Arc<RequestTable>) -> RemoteSignInPrompting {
        RemoteSignInPrompting { table }
    }
}

#[async_trait]
impl SignInPrompting for RemoteSignInPrompting {
    async fn prepare(
        &self,
        title: String,
        help: Option<CredentialHelp>,
        values: BTreeMap<String, String>,
        error: Option<String>,
    ) {
        let request = SignInPrepareRequest {
            title,
            help: help.map(|help| WireCredentialHelp {
                text: help.text,
                link_title: help.link_title,
                url: help.url.map(|url| url.as_str().to_string()),
            }),
            values,
            error,
        };
        let _ = self
            .table
            .call::<IgnoredAny>(
                "service.signIn.prepare",
                Some(wire(&request)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await;
    }

    async fn last_non_secret_values(&self) -> Option<BTreeMap<String, String>> {
        let reply = self
            .table
            .call::<SignInLastValuesReply>(
                "service.signIn.lastNonSecretValues",
                None,
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await
            .ok()?;
        reply.values
    }
}

// MARK: - Text generation

/// The host's text model (present only when the config asks for text generation).
pub struct RemoteTextGenerator {
    table: Arc<RequestTable>,
}

impl RemoteTextGenerator {
    /// Creates the generator, sending its requests through `table`.
    pub fn new(table: Arc<RequestTable>) -> RemoteTextGenerator {
        RemoteTextGenerator { table }
    }
}

#[async_trait]
impl TextGenerator for RemoteTextGenerator {
    async fn availability(&self) -> TextGeneratorAvailability {
        let reply: TextAvailabilityReply = match self
            .table
            .call(
                "service.text.availability",
                None,
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await
        {
            Ok(reply) => reply,
            Err(_) => {
                return TextGeneratorAvailability::TemporarilyUnavailable {
                    reason: "the host did not answer".to_string(),
                };
            }
        };
        match reply.kind.as_str() {
            "available" => {
                match (reply.engine_id, reply.engine_name) {
                    (Some(id), Some(name)) => TextGeneratorAvailability::Available(
                        EngineInfo::new(id, name, reply.is_on_device.unwrap_or(false)),
                    ),
                    _ => TextGeneratorAvailability::Unavailable {
                        reason: "the host named no engine".to_string(),
                    },
                }
            }
            "temporarilyUnavailable" => TextGeneratorAvailability::TemporarilyUnavailable {
                reason: reply.reason.unwrap_or_default(),
            },
            _ => TextGeneratorAvailability::Unavailable {
                reason: reply.reason.unwrap_or_default(),
            },
        }
    }

    async fn generate(&self, instructions: &str, prompt: &str) -> Outcome<String> {
        let request = TextGenerateRequest {
            instructions: instructions.to_string(),
            prompt: prompt.to_string(),
        };
        let reply: TextGenerateReply = self
            .table
            .call(
                "service.text.generate",
                Some(wire(&request)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await?;
        Ok(reply.text)
    }
}

// MARK: - Host-supplied access tokens

/// The host's own account service (Windows' Web Account Manager) as the library's `ExternalTokenSource`, over
/// `service.token.*`. Present only when the config sets `externalTokens`.
///
/// Silent requests time out after `HOST_SERVICE_TIMEOUT` on the engine clock. A sign-in has no timeout: it ends with the
/// host's answer or with a cancellation, which tells the host to close its dialog (`service.token.cancelSignIn`, best
/// effort). Nothing a diagnostic carries comes from the host except a `reason` that is one of the documented codes;
/// tokens, account ids, emails and the host's error text never reach one or a returned error.
pub struct RemoteExternalTokenSource {
    table: Arc<RequestTable>,
    diagnostics: EngineDiagnosticSink,
}

impl RemoteExternalTokenSource {
    /// The `reason` codes a host may send, as documented in `docs/engine-protocol.md`.
    pub const DOCUMENTED_REASONS: [&'static str; 8] = [
        "consent",
        "signedOut",
        "mfa",
        "policy",
        "accountMissing",
        "adminConsentRequired",
        "brokerUnavailable",
        "other",
    ];

    /// Creates the token source, sending its requests through `table` and reporting problems to `diagnostics`.
    pub fn new(
        table: Arc<RequestTable>,
        diagnostics: EngineDiagnosticSink,
    ) -> RemoteExternalTokenSource {
        RemoteExternalTokenSource { table, diagnostics }
    }

    fn incomplete() -> Failure {
        Failure::Source(SourceError::InvalidResponse(
            "the host's token answer was incomplete".to_string(),
        ))
    }

    fn unavailable() -> Failure {
        Failure::Source(SourceError::InvalidResponse(
            "Windows sign-in is not available".to_string(),
        ))
    }

    fn token(
        access_token: Option<String>,
        expires_at: Option<String>,
    ) -> Outcome<ExternalAccessToken> {
        let (Some(access_token), Some(expires_at)) = (access_token, expires_at) else {
            return Err(Self::incomplete());
        };
        if access_token.is_empty() {
            return Err(Self::incomplete());
        }
        let Some(date) = WireDates::date(&expires_at) else {
            return Err(Self::incomplete());
        };
        Ok(ExternalAccessToken::new(access_token, date))
    }

    /// The host's error text is not used: it could carry account data.
    fn failure(error: RequestError) -> Failure {
        match error {
            RequestError::Cancelled => Failure::Cancelled,
            RequestError::Service(error) => match error.kind {
                ReplyErrorKind::Network => Failure::Source(SourceError::Network(
                    "Windows sign-in could not reach the network".to_string(),
                )),
                ReplyErrorKind::NotFound | ReplyErrorKind::Denied => Self::unavailable(),
                ReplyErrorKind::Failed | ReplyErrorKind::Cancelled => Failure::Source(
                    SourceError::InvalidResponse("the Windows account service failed".to_string()),
                ),
            },
        }
    }

    fn note_reason(&self, reason: Option<&str>) {
        let Some(reason) = reason else { return };
        if !Self::DOCUMENTED_REASONS.contains(&reason) {
            return;
        }
        (self.diagnostics)(EngineDiagnostic::new(
            Level::Info,
            "tokens",
            format!("the host reported reason {reason}"),
        ));
    }
}

/// Tells the host to close its sign-in dialog when the sign-in is dropped before the host answered (best effort, and
/// only while the dialog may still be open: an answer that already arrived needs no cancel).
struct SignInCancelGuard {
    table: Arc<RequestTable>,
    sign_in_id: String,
    answered: bool,
}

impl Drop for SignInCancelGuard {
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        let table = self.table.clone();
        let request = wire(&TokenCancelSignInRequest {
            sign_in_id: std::mem::take(&mut self.sign_in_id),
        });
        spawn_detached(async move {
            let _ = table
                .call::<IgnoredAny>(
                    "service.token.cancelSignIn",
                    Some(request),
                    Some(HOST_SERVICE_TIMEOUT),
                )
                .await;
        });
    }
}

#[async_trait]
impl ExternalTokenSource for RemoteExternalTokenSource {
    async fn access_token(
        &self,
        account: &ExternalAccount,
        scopes: &[String],
        force_refresh: bool,
    ) -> Outcome<ExternalAccessToken> {
        let request = TokenGetRequest {
            connection_id: account.connection_id.clone().unwrap_or_default(),
            account_id: account.id.clone(),
            email: account.email.clone(),
            scopes: scopes.to_vec(),
            force_refresh,
        };
        let reply: TokenGetReply = self
            .table
            .call(
                "service.token.get",
                Some(wire(&request)),
                Some(HOST_SERVICE_TIMEOUT),
            )
            .await
            .map_err(Self::failure)?;
        self.note_reason(reply.reason.as_deref());
        match reply.status.as_str() {
            "ok" => Self::token(reply.access_token, reply.expires_at),
            "interactionRequired" => Err(Failure::Source(SourceError::AuthExpired)),
            "unavailable" => Err(Self::unavailable()),
            _ => Err(Self::incomplete()),
        }
    }

    async fn sign_in(&self, request: &ExternalSignInRequest) -> Outcome<ExternalSignInResult> {
        let sign_in_id = uuid_string(Uuid::new_v4());
        let email = request
            .email
            .clone()
            .or_else(|| request.account.as_ref().and_then(|a| a.email.clone()));
        let wire_request = TokenSignInRequest {
            sign_in_id: sign_in_id.clone(),
            scopes: request.scopes.clone(),
            account_id: request.account.as_ref().map(|a| a.id.clone()),
            email: email.clone(),
        };
        let mut guard = SignInCancelGuard {
            table: self.table.clone(),
            sign_in_id,
            answered: false,
        };
        let result: Result<TokenSignInReply, RequestError> = self
            .table
            .call("service.token.signIn", Some(wire(&wire_request)), None)
            .await;
        guard.answered = true;
        let reply = result.map_err(Self::failure)?;
        self.note_reason(reply.reason.as_deref());
        match reply.status.as_str() {
            "ok" => {
                let account_id = match reply.account_id {
                    Some(id) if !id.is_empty() => id,
                    _ => return Err(Self::incomplete()),
                };
                Ok(ExternalSignInResult::new(
                    ExternalAccount {
                        id: account_id,
                        email,
                        connection_id: request
                            .account
                            .as_ref()
                            .and_then(|a| a.connection_id.clone()),
                    },
                    Self::token(reply.access_token, reply.expires_at)?,
                ))
            }
            "blocked" => Err(Failure::Source(SourceError::InvalidResponse(
                "admin approval is required".to_string(),
            ))),
            "unavailable" => Err(Self::unavailable()),
            _ => Err(Self::incomplete()),
        }
    }
}
