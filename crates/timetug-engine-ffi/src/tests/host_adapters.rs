//! The host adapters, with Rust fakes standing in for the foreign side: each adapter forwards every method, converts
//! values both ways and maps host errors per service; a host that returns an error and a host whose call never completes
//! (dropped by the caller) are both covered.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use calendar_core::connection::{
    AuthorizationInteraction as _, CredentialField as EngineField,
    CredentialSnapshot as EngineSnapshot, CredentialStore as _,
};
use calendar_core::external_tokens::{
    ExternalAccount as EngineAccount, ExternalSignInRequest as EngineSignIn,
    ExternalTokenSource as _,
};
use calendar_core::failure::Failure;
use calendar_core::http::{HttpRequest as EngineRequest, HttpTransport as _};
use calendar_core::model::{CalendarEvent as EngineEvent, ResponseStatus as EngineResponse};
use calendar_core::source_types::{
    CalendarChange, CalendarSource as _, SeriesSource as _, SourceError as EngineSourceError,
};
use calendar_core::time::DateInterval;
use calendar_core::url_string::UrlString;
use calendar_core::write::event_draft::{EventDraft as EngineDraft, EventTiming};
use calendar_core::write::event_patch::EventPatch as EnginePatch;
use calendar_core::write::types::{
    EventRef as EngineRef, NotifyPolicy as EngineNotify, RecurrenceScope as EngineScope,
    WriteError as EngineWriteError,
};
use calendar_core::write::writable_source::WritableCalendarSource as _;
use jiff::Timestamp;
use timetug_engine::host::SignInPrompting as _;
use timetug_engine::inference::text_generator::{
    TextGenerator as _, TextGeneratorAvailability as EngineAvailability,
};
use timetug_engine::settings::{EngineSettings as EngineEngineSettings, SettingsStorage as _};
use uuid::Uuid;

use crate::errors::{FfiError, HostError, SourceError};
use crate::host::authorization::{
    AuthorizationInteraction, AuthorizationInteractionAdapter, OAuthRedirectHandle,
    OAuthRedirectSession,
};
use crate::host::credentials::{CredentialStore, CredentialStoreAdapter};
use crate::host::diagnostics::{DiagnosticSink, engine_sink};
use crate::host::external_tokens::{ExternalTokenSource, ExternalTokenSourceAdapter};
use crate::host::http::{HttpTransport, HttpTransportAdapter};
use crate::host::settings_storage::{SettingsStorage, SettingsStorageAdapter};
use crate::host::sign_in::{SignInPrompting, SignInPromptingAdapter};
use crate::host::system_calendar::{
    SystemCalendarAdapter, SystemCalendarSource, SystemChangeHandler,
};
use crate::host::text_generator::{TextGenerator, TextGeneratorAdapter};
use crate::values::accounts::{CredentialField, CredentialHelp, CredentialSnapshot};
use crate::values::diagnostics::{EngineDiagnostic, EngineDiagnosticLevel};
use crate::values::host::{
    ExternalAccessToken, ExternalAccount, ExternalSignInRequest, ExternalSignInResult, HttpRequest,
    HttpResponse, TextGeneratorAvailability,
};
use crate::values::model::{
    CalendarDescriptor, CalendarEvent, CalendarSeries, ResponseStatus, SourceCapabilities,
};
use crate::values::settings::EngineSettings;
use crate::values::write::{EventDraft, EventPatch, EventRef, NotifyPolicy, RecurrenceScope};
use crate::values::{ToEngine, ToMirror};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap()
}

fn url(text: &str) -> UrlString {
    UrlString::parse(text).unwrap()
}

/// What a fake does on its next call.
#[derive(Clone)]
enum Behaviour {
    Succeed,
    Fail(HostError),
    Never,
}

/// Records what a never-returning call left behind: it started, and its future was dropped.
#[derive(Default)]
struct Probe {
    started: AtomicUsize,
    dropped: AtomicUsize,
}

struct DropFlag(Arc<Probe>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// Runs the host side of a call according to the behaviour: returns the value, fails, or never completes.
async fn behave<T>(
    behaviour: &Behaviour,
    probe: &Arc<Probe>,
    value: impl FnOnce() -> T,
) -> Result<T, HostError> {
    match behaviour {
        Behaviour::Succeed => Ok(value()),
        Behaviour::Fail(error) => Err(error.clone()),
        Behaviour::Never => {
            probe.started.fetch_add(1, Ordering::SeqCst);
            let _flag = DropFlag(probe.clone());
            std::future::pending::<()>().await;
            unreachable!()
        }
    }
}

// ---------- credentials ----------

struct FakeCredentials {
    behaviour: Mutex<Behaviour>,
    probe: Arc<Probe>,
    calls: Mutex<Vec<String>>,
}

impl FakeCredentials {
    fn new(behaviour: Behaviour) -> Arc<FakeCredentials> {
        Arc::new(FakeCredentials {
            behaviour: Mutex::new(behaviour),
            probe: Arc::default(),
            calls: Mutex::default(),
        })
    }
    fn note(&self, text: String) {
        self.calls.lock().unwrap().push(text);
    }
    fn behaviour(&self) -> Behaviour {
        self.behaviour.lock().unwrap().clone()
    }
}

#[async_trait]
impl CredentialStore for FakeCredentials {
    async fn secrets(
        &self,
        connection_id: String,
    ) -> Result<Option<HashMap<String, String>>, HostError> {
        self.note(format!("secrets {connection_id}"));
        behave(&self.behaviour(), &self.probe, || {
            Some(HashMap::from([("token".to_string(), "t".to_string())]))
        })
        .await
    }
    async fn set_secrets(
        &self,
        secrets: HashMap<String, String>,
        connection_id: String,
    ) -> Result<(), HostError> {
        self.note(format!("set {connection_id} {}", secrets.len()));
        behave(&self.behaviour(), &self.probe, || ()).await
    }
    async fn remove_secrets(&self, connection_id: String) -> Result<(), HostError> {
        self.note(format!("remove {connection_id}"));
        behave(&self.behaviour(), &self.probe, || ()).await
    }
    async fn credential_snapshot(
        &self,
        connection_id: String,
    ) -> Result<Option<CredentialSnapshot>, HostError> {
        self.note(format!("snapshot {connection_id}"));
        behave(&self.behaviour(), &self.probe, || {
            Some(CredentialSnapshot {
                secrets: HashMap::from([("k".into(), "v".into())]),
                revision: Uuid::from_u128(5),
            })
        })
        .await
    }
    async fn update_refresh_token(
        &self,
        token: String,
        connection_id: String,
        expected_revision: Uuid,
    ) -> Result<bool, HostError> {
        self.note(format!(
            "refresh {token} {connection_id} {expected_revision}"
        ));
        behave(&self.behaviour(), &self.probe, || true).await
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_credential_adapter_forwards_every_method_and_converts_values() {
    let fake = FakeCredentials::new(Behaviour::Succeed);
    let adapter = CredentialStoreAdapter::new(fake.clone());
    assert_eq!(
        adapter.secrets("c1").await.unwrap(),
        Some(BTreeMap::from([("token".to_string(), "t".to_string())]))
    );
    adapter
        .set_secrets(BTreeMap::from([("a".to_string(), "b".to_string())]), "c1")
        .await
        .unwrap();
    adapter.remove_secrets("c1").await.unwrap();
    let snapshot = adapter.credential_snapshot("c1").await.unwrap().unwrap();
    assert_eq!(
        snapshot,
        EngineSnapshot::new(
            BTreeMap::from([("k".to_string(), "v".to_string())]),
            Uuid::from_u128(5)
        )
    );
    assert!(
        adapter
            .update_refresh_token("tok", "c1", Uuid::from_u128(9))
            .await
            .unwrap()
    );
    assert_eq!(
        *fake.calls.lock().unwrap(),
        vec![
            "secrets c1".to_string(),
            "set c1 1".to_string(),
            "remove c1".to_string(),
            "snapshot c1".to_string(),
            format!("refresh tok c1 {}", Uuid::from_u128(9)),
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_credential_host_that_throws_becomes_a_failure_for_every_method() {
    for (error, expected) in [
        (HostError::Denied, Failure::Host("denied".into())),
        (
            HostError::Network {
                detail: "x".into(),
            },
            Failure::Host("network: x".into()),
        ),
        (HostError::Cancelled, Failure::Cancelled),
        (
            HostError::Failed {
                detail: String::new(),
            },
            Failure::Host("failed".into()),
        ),
    ] {
        let adapter =
            CredentialStoreAdapter::new(FakeCredentials::new(Behaviour::Fail(error.clone())));
        assert_eq!(
            adapter.secrets("c").await.unwrap_err(),
            expected,
            "{error:?}"
        );
        assert_eq!(
            adapter.set_secrets(BTreeMap::new(), "c").await.unwrap_err(),
            expected
        );
        assert_eq!(adapter.remove_secrets("c").await.unwrap_err(), expected);
        assert_eq!(
            adapter.credential_snapshot("c").await.unwrap_err(),
            expected
        );
        assert_eq!(
            adapter
                .update_refresh_token("t", "c", Uuid::nil())
                .await
                .unwrap_err(),
            expected
        );
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_credential_call_that_never_returns_is_dropped_by_the_caller() {
    let fake = FakeCredentials::new(Behaviour::Never);
    let adapter = CredentialStoreAdapter::new(fake.clone());
    let outcome = tokio::time::timeout(Duration::from_secs(5), adapter.secrets("c")).await;
    assert!(outcome.is_err(), "the call must still be pending");
    assert_eq!(fake.probe.started.load(Ordering::SeqCst), 1);
    assert_eq!(
        fake.probe.dropped.load(Ordering::SeqCst),
        1,
        "the host's future was dropped with the caller's"
    );
}

// ---------- authorization ----------

struct FakeSession {
    closed: AtomicUsize,
    behaviour: Behaviour,
    probe: Arc<Probe>,
}

#[async_trait]
impl OAuthRedirectSession for FakeSession {
    fn redirect_uri(&self) -> UrlString {
        url("http://127.0.0.1:5000/callback")
    }
    async fn authorize(&self, authorization_url: UrlString) -> Result<UrlString, HostError> {
        let seen = authorization_url.as_str().to_string();
        behave(&self.behaviour, &self.probe, || {
            url(&format!("{}?code=abc", seen.split('?').next().unwrap()))
        })
        .await
    }
    async fn release(&self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

struct FakeInteraction {
    session: Arc<FakeSession>,
    begin: Behaviour,
    prompt: Behaviour,
    probe: Arc<Probe>,
}

#[async_trait]
impl AuthorizationInteraction for FakeInteraction {
    async fn begin_oauth_redirect(&self) -> Result<OAuthRedirectHandle, HostError> {
        behave(&self.begin, &self.probe, || OAuthRedirectHandle {
            session: self.session.clone(),
        })
        .await
    }
    async fn prompt_credentials(
        &self,
        fields: Vec<CredentialField>,
    ) -> Result<HashMap<String, String>, HostError> {
        behave(&self.prompt, &self.probe, || {
            fields
                .iter()
                .map(|f| (f.key.clone(), format!("typed-{}", f.label)))
                .collect()
        })
        .await
    }
}

fn interaction(
    session_behaviour: Behaviour,
    begin: Behaviour,
    prompt: Behaviour,
) -> Arc<FakeInteraction> {
    Arc::new(FakeInteraction {
        session: Arc::new(FakeSession {
            closed: AtomicUsize::new(0),
            behaviour: session_behaviour,
            probe: Arc::default(),
        }),
        begin,
        prompt,
        probe: Arc::default(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn the_authorization_adapter_forwards_the_redirect_session_and_the_form() {
    let fake = interaction(Behaviour::Succeed, Behaviour::Succeed, Behaviour::Succeed);
    let adapter = AuthorizationInteractionAdapter::new(fake.clone());
    let session = adapter.begin_oauth_redirect().await.unwrap();
    assert_eq!(
        session.redirect_uri(),
        url("http://127.0.0.1:5000/callback")
    );
    let returned = session
        .authorize(&url("https://accounts.example.com/auth?x=1"))
        .await
        .unwrap();
    assert_eq!(returned, url("https://accounts.example.com/auth?code=abc"));
    session.close().await;
    assert_eq!(fake.session.closed.load(Ordering::SeqCst), 1);
    let fields = vec![EngineField {
        key: "user".into(),
        label: "User".into(),
        is_secret: false,
    }];
    assert_eq!(
        adapter.prompt_credentials(&fields).await.unwrap(),
        BTreeMap::from([("user".to_string(), "typed-User".to_string())])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn authorization_failures_map_like_the_general_service() {
    let adapter = AuthorizationInteractionAdapter::new(interaction(
        Behaviour::Fail(HostError::Cancelled),
        Behaviour::Fail(HostError::Denied),
        Behaviour::Fail(HostError::Cancelled),
    ));
    assert_eq!(
        adapter.begin_oauth_redirect().await.err(),
        Some(Failure::Host("denied".into()))
    );
    assert_eq!(
        adapter.prompt_credentials(&[]).await.unwrap_err(),
        Failure::Cancelled
    );
    let ok = AuthorizationInteractionAdapter::new(interaction(
        Behaviour::Fail(HostError::Cancelled),
        Behaviour::Succeed,
        Behaviour::Succeed,
    ));
    let session = ok.begin_oauth_redirect().await.unwrap();
    assert_eq!(
        session
            .authorize(&url("https://a.example/"))
            .await
            .unwrap_err(),
        Failure::Cancelled
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn an_authorize_that_never_returns_is_dropped_and_the_session_can_still_be_closed() {
    let fake = interaction(Behaviour::Never, Behaviour::Succeed, Behaviour::Never);
    let adapter = AuthorizationInteractionAdapter::new(fake.clone());
    let session = adapter.begin_oauth_redirect().await.unwrap();
    let pending = tokio::time::timeout(
        Duration::from_secs(30),
        session.authorize(&url("https://a.example/")),
    )
    .await;
    assert!(pending.is_err());
    assert_eq!(fake.session.probe.dropped.load(Ordering::SeqCst), 1);
    // The engine's drop guard closes the session after the cancelled sign-in.
    session.close().await;
    assert_eq!(fake.session.closed.load(Ordering::SeqCst), 1);
    assert!(
        tokio::time::timeout(Duration::from_secs(30), adapter.prompt_credentials(&[]))
            .await
            .is_err()
    );
    assert_eq!(fake.probe.dropped.load(Ordering::SeqCst), 1);
}

// ---------- sign-in form, settings, diagnostics ----------

type Prepared = (
    String,
    Option<CredentialHelp>,
    HashMap<String, String>,
    Option<String>,
);

#[derive(Default)]
struct FakePrompt {
    prepared: Mutex<Vec<Prepared>>,
}

#[async_trait]
impl SignInPrompting for FakePrompt {
    async fn prepare(
        &self,
        title: String,
        help: Option<CredentialHelp>,
        values: HashMap<String, String>,
        error: Option<String>,
    ) {
        self.prepared
            .lock()
            .unwrap()
            .push((title, help, values, error));
    }
    async fn last_non_secret_values(&self) -> Option<HashMap<String, String>> {
        Some(HashMap::from([("user".to_string(), "ann".to_string())]))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_sign_in_adapter_forwards_prepare_and_the_last_values() {
    let fake = Arc::new(FakePrompt::default());
    let adapter = SignInPromptingAdapter::new(fake.clone());
    let help = calendar_core::connection::CredentialHelp {
        text: "app password".into(),
        link_title: Some("How".into()),
        url: UrlString::parse("https://example.com/h"),
    };
    adapter
        .prepare(
            "iCloud".into(),
            Some(help.clone()),
            BTreeMap::from([("user".to_string(), "a".to_string())]),
            Some("bad".into()),
        )
        .await;
    let prepared = fake.prepared.lock().unwrap().clone();
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].0, "iCloud");
    assert_eq!(prepared[0].1.clone().unwrap().to_engine(), help);
    assert_eq!(prepared[0].3.as_deref(), Some("bad"));
    assert_eq!(
        adapter.last_non_secret_values().await,
        Some(BTreeMap::from([("user".to_string(), "ann".to_string())]))
    );
}

#[derive(Default)]
struct FakeSettings {
    stored: Mutex<Option<EngineSettings>>,
}

impl SettingsStorage for FakeSettings {
    fn load(&self) -> EngineSettings {
        self.stored
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| EngineEngineSettings::default().to_mirror())
    }
    fn save(&self, settings: EngineSettings) {
        *self.stored.lock().unwrap() = Some(settings);
    }
}

#[test]
fn the_settings_adapter_loads_defaults_and_saves_what_the_engine_gives_it() {
    let fake = Arc::new(FakeSettings::default());
    let adapter = SettingsStorageAdapter::new(fake.clone());
    assert_eq!(adapter.load(), EngineEngineSettings::default());
    let mut changed = EngineEngineSettings::default();
    changed.takeover.lead_time = 99.0;
    changed.inference_enabled = true;
    adapter.save(&changed);
    assert_eq!(adapter.load(), changed);
}

struct CollectingSink(Mutex<Vec<EngineDiagnostic>>);
impl DiagnosticSink for CollectingSink {
    fn log(&self, diagnostic: EngineDiagnostic) {
        self.0.lock().unwrap().push(diagnostic);
    }
}

#[test]
fn the_diagnostic_sink_gets_each_line_in_order() {
    let host = Arc::new(CollectingSink(Mutex::default()));
    let sink = engine_sink(host.clone());
    sink(timetug_engine::diagnostic::EngineDiagnostic::new(
        timetug_engine::diagnostic::Level::Warning,
        "a",
        "first",
    ));
    sink(timetug_engine::diagnostic::EngineDiagnostic::new(
        timetug_engine::diagnostic::Level::Error,
        "b",
        "second",
    ));
    let lines = host.0.lock().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        EngineDiagnostic {
            level: EngineDiagnosticLevel::Warning,
            category: "a".into(),
            message: "first".into()
        }
    );
    assert_eq!(lines[1].message, "second");
}

// ---------- http, text, tokens ----------

struct FakeHttp {
    behaviour: Behaviour,
    probe: Arc<Probe>,
    seen: Mutex<Vec<HttpRequest>>,
}

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HostError> {
        self.seen.lock().unwrap().push(request);
        behave(&self.behaviour, &self.probe, || HttpResponse {
            status: 200,
            headers: HashMap::from([("Content-Type".to_string(), "text/plain".to_string())]),
            body: b"hello".to_vec(),
        })
        .await
    }
}

fn fake_http(behaviour: Behaviour) -> Arc<FakeHttp> {
    Arc::new(FakeHttp {
        behaviour,
        probe: Arc::default(),
        seen: Mutex::default(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn the_http_adapter_sends_the_request_and_lower_cases_response_headers() {
    let fake = fake_http(Behaviour::Succeed);
    let adapter = HttpTransportAdapter::new(fake.clone());
    let mut request = EngineRequest::new(url("https://api.example.com/x?y=\u{e9}"));
    request.method = "POST".into();
    request
        .headers
        .insert("Authorization".into(), "Bearer t".into());
    request.body = Some(vec![0, 1, 255]);
    let response = adapter.send(request).await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/plain"));
    assert_eq!(response.body, b"hello".to_vec());
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].body, Some(vec![0, 1, 255]));
    assert_eq!(seen[0].url.as_str(), "https://api.example.com/x?y=\u{e9}");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn http_failures_are_network_errors_and_a_never_returning_send_is_dropped() {
    for (error, expected) in [
        (
            HostError::Network {
                detail: "offline".into(),
            },
            Failure::Source(EngineSourceError::Network("network: offline".into())),
        ),
        (
            HostError::Denied,
            Failure::Source(EngineSourceError::Network("denied".into())),
        ),
        (HostError::Cancelled, Failure::Cancelled),
    ] {
        let adapter = HttpTransportAdapter::new(fake_http(Behaviour::Fail(error.clone())));
        assert_eq!(
            adapter
                .send(EngineRequest::new(url("https://a.example/")))
                .await
                .unwrap_err(),
            expected,
            "{error:?}"
        );
    }
    let fake = fake_http(Behaviour::Never);
    let adapter = HttpTransportAdapter::new(fake.clone());
    assert!(
        tokio::time::timeout(
            Duration::from_secs(60),
            adapter.send(EngineRequest::new(url("https://a.example/")))
        )
        .await
        .is_err()
    );
    assert_eq!(fake.probe.dropped.load(Ordering::SeqCst), 1);
}

struct FakeText {
    behaviour: Behaviour,
    probe: Arc<Probe>,
}

#[async_trait]
impl TextGenerator for FakeText {
    async fn availability(&self) -> TextGeneratorAvailability {
        TextGeneratorAvailability::TemporarilyUnavailable {
            reason: "warming up".into(),
        }
    }
    async fn generate(&self, instructions: String, prompt: String) -> Result<String, HostError> {
        behave(&self.behaviour, &self.probe, || {
            format!("{instructions}|{prompt}")
        })
        .await
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_text_adapter_forwards_availability_and_generation() {
    let fake = Arc::new(FakeText {
        behaviour: Behaviour::Succeed,
        probe: Arc::default(),
    });
    let adapter = TextGeneratorAdapter::new(fake);
    assert_eq!(
        adapter.availability().await,
        EngineAvailability::TemporarilyUnavailable {
            reason: "warming up".into()
        }
    );
    assert_eq!(adapter.generate("i", "p").await.unwrap(), "i|p");
    let failing = TextGeneratorAdapter::new(Arc::new(FakeText {
        behaviour: Behaviour::Fail(HostError::Failed {
            detail: "no".into(),
        }),
        probe: Arc::default(),
    }));
    assert_eq!(
        failing.generate("i", "p").await.unwrap_err(),
        Failure::Host("failed: no".into())
    );
    let never = Arc::new(FakeText {
        behaviour: Behaviour::Never,
        probe: Arc::default(),
    });
    let adapter = TextGeneratorAdapter::new(never.clone());
    assert!(
        tokio::time::timeout(Duration::from_secs(60), adapter.generate("i", "p"))
            .await
            .is_err()
    );
    assert_eq!(never.probe.dropped.load(Ordering::SeqCst), 1);
}

struct FakeTokens {
    behaviour: Behaviour,
    probe: Arc<Probe>,
}

#[async_trait]
impl ExternalTokenSource for FakeTokens {
    async fn access_token(
        &self,
        account: ExternalAccount,
        scopes: Vec<String>,
        force_refresh: bool,
    ) -> Result<ExternalAccessToken, HostError> {
        behave(&self.behaviour, &self.probe, || ExternalAccessToken {
            access_token: format!("{}-{}-{force_refresh}", account.id, scopes.join(",")),
            expires_at: at(100),
        })
        .await
    }
    async fn sign_in(
        &self,
        request: ExternalSignInRequest,
    ) -> Result<ExternalSignInResult, HostError> {
        behave(&self.behaviour, &self.probe, || ExternalSignInResult {
            account: ExternalAccount {
                id: "acct".into(),
                email: request.email.clone(),
                connection_id: None,
            },
            token: ExternalAccessToken {
                access_token: "t".into(),
                expires_at: at(200),
            },
        })
        .await
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_token_adapter_forwards_and_maps_errors_as_the_token_service() {
    let adapter = ExternalTokenSourceAdapter::new(Arc::new(FakeTokens {
        behaviour: Behaviour::Succeed,
        probe: Arc::default(),
    }));
    let token = adapter
        .access_token(
            &EngineAccount::new("a1"),
            &["s1".to_string(), "s2".to_string()],
            true,
        )
        .await
        .unwrap();
    assert_eq!(token.access_token, "a1-s1,s2-true");
    assert_eq!(token.expires_at, at(100));
    let mut request = EngineSignIn::new(vec!["s".into()]);
    request.email = Some("ann@example.com".into());
    let result = adapter.sign_in(&request).await.unwrap();
    assert_eq!(result.account.email.as_deref(), Some("ann@example.com"));
    let denied = ExternalTokenSourceAdapter::new(Arc::new(FakeTokens {
        behaviour: Behaviour::Fail(HostError::Denied),
        probe: Arc::default(),
    }));
    assert_eq!(
        denied
            .access_token(&EngineAccount::new("a"), &[], false)
            .await
            .unwrap_err(),
        Failure::Source(EngineSourceError::InvalidResponse(
            "Windows sign-in is not available".into()
        ))
    );
    let never = Arc::new(FakeTokens {
        behaviour: Behaviour::Never,
        probe: Arc::default(),
    });
    let adapter = ExternalTokenSourceAdapter::new(never.clone());
    assert!(
        tokio::time::timeout(Duration::from_secs(60), adapter.sign_in(&request))
            .await
            .is_err()
    );
    assert_eq!(never.probe.dropped.load(Ordering::SeqCst), 1);
}

// ---------- the system calendar source ----------

struct FakeSystem {
    handler: Mutex<Option<Arc<SystemChangeHandler>>>,
    order: Mutex<Vec<String>>,
    error: Mutex<Option<FfiError>>,
}

impl FakeSystem {
    fn new() -> Arc<FakeSystem> {
        Arc::new(FakeSystem {
            handler: Mutex::default(),
            order: Mutex::default(),
            error: Mutex::default(),
        })
    }
    fn note(&self, text: &str) {
        self.order.lock().unwrap().push(text.to_string());
    }
    fn result<T>(&self, value: T) -> Result<T, FfiError> {
        match self.error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }
}

fn event() -> EngineEvent {
    EngineEvent::new("e", "c", "Title", at(1_700_000_000), at(1_700_003_600))
}

#[async_trait]
impl SystemCalendarSource for FakeSystem {
    fn id(&self) -> String {
        "eventkit".into()
    }
    fn display_name(&self) -> String {
        "Apple Calendar".into()
    }
    fn capabilities(&self) -> SourceCapabilities {
        calendar_core::source_types::SourceCapabilities {
            can_write: true,
            can_edit_attendees: false,
            can_respond_to_invite: true,
            provided_fields: Default::default(),
            sync_kind: calendar_core::source_types::SyncKind::Notification,
            supports_push: true,
            writable_fields: Default::default(),
            controls_notifications: false,
            recurrence_scopes: Default::default(),
        }
        .to_mirror()
    }
    async fn request_access(&self) {
        self.note("access");
    }
    async fn calendars(&self) -> Result<Vec<CalendarDescriptor>, FfiError> {
        self.note("calendars");
        self.result(vec![
            calendar_core::model::CalendarDescriptor::new(
                "c",
                "Calendar",
                calendar_core::model::CalendarService::event_kit(),
            )
            .to_mirror(),
        ])
    }
    async fn events_in(
        &self,
        start: Timestamp,
        end: Timestamp,
    ) -> Result<Vec<CalendarEvent>, FfiError> {
        self.note(&format!("events {} {}", start.as_second(), end.as_second()));
        self.result(vec![event().to_mirror()])
    }
    async fn series(&self, id: String, calendar_id: String) -> Result<CalendarSeries, FfiError> {
        self.note(&format!("series {id} {calendar_id}"));
        self.result(
            calendar_core::recurrence::set::CalendarSeries {
                series_id: id,
                calendar_id,
                start: at(1),
                time_zone: calendar_core::time::Zone::utc(),
                is_all_day: false,
                recurrence: calendar_core::recurrence::set::RecurrenceSet {
                    rules: vec![],
                    extra_dates: None,
                    excluded_dates: None,
                    unparsed: vec![],
                },
            }
            .to_mirror(),
        )
    }
    async fn create(
        &self,
        draft: EventDraft,
        calendar_id: String,
        notify: NotifyPolicy,
    ) -> Result<CalendarEvent, FfiError> {
        self.note(&format!("create {} {calendar_id} {notify:?}", draft.title));
        self.result(event().to_mirror())
    }
    async fn update(
        &self,
        reference: EventRef,
        patch: EventPatch,
        scope: RecurrenceScope,
        notify: NotifyPolicy,
    ) -> Result<CalendarEvent, FfiError> {
        self.note(&format!(
            "update {} {:?} {scope:?} {notify:?}",
            reference.event_id, patch.title
        ));
        self.result(event().to_mirror())
    }
    async fn delete(
        &self,
        reference: EventRef,
        scope: RecurrenceScope,
        notify: NotifyPolicy,
    ) -> Result<(), FfiError> {
        self.note(&format!(
            "delete {} {scope:?} {notify:?}",
            reference.event_id
        ));
        self.result(())
    }
    async fn respond(
        &self,
        reference: EventRef,
        response: ResponseStatus,
        scope: RecurrenceScope,
        notify: NotifyPolicy,
    ) -> Result<CalendarEvent, FfiError> {
        self.note(&format!(
            "respond {} {response:?} {scope:?} {notify:?}",
            reference.event_id
        ));
        self.result(event().to_mirror())
    }
    async fn set_change_handler(&self, handler: Arc<SystemChangeHandler>) {
        self.note("handler");
        *self.handler.lock().unwrap() = Some(handler);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_system_adapter_gives_the_host_its_handler_before_the_first_read_and_only_once() {
    let fake = FakeSystem::new();
    let adapter = SystemCalendarAdapter::new(fake.clone());
    assert_eq!(adapter.id(), "eventkit");
    assert_eq!(adapter.display_name(), "Apple Calendar");
    assert!(adapter.capabilities().can_write);
    let calendars = adapter.calendars().await.unwrap();
    assert_eq!(calendars[0].id, "c");
    let events = adapter
        .events_in(DateInterval::new(at(10), at(20)))
        .await
        .unwrap();
    assert_eq!(events, vec![event()]);
    adapter.calendars().await.unwrap();
    assert_eq!(
        *fake.order.lock().unwrap(),
        vec!["handler", "calendars", "events 10 20", "calendars"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_system_adapter_forwards_series_and_every_write() {
    let fake = FakeSystem::new();
    let adapter = SystemCalendarAdapter::new(fake.clone());
    assert!(adapter.as_series().is_some() && adapter.as_writable().is_some());
    let series = adapter.series("s1", "c1").await.unwrap();
    assert_eq!(
        (series.series_id.as_str(), series.calendar_id.as_str()),
        ("s1", "c1")
    );
    let timing = EventTiming::new(at(1), at(2), None, false);
    let draft = EngineDraft {
        title: "New".into(),
        notes: None,
        location: None,
        timing,
        availability: calendar_core::model::Availability::Busy,
        visibility: calendar_core::model::Visibility::Default,
        reminders: None,
        attendees: vec![],
        conference: calendar_core::write::types::ConferenceRequest::None,
        recurrence: None,
        uid: None,
    };
    assert_eq!(
        adapter
            .create(&draft, "c1", EngineNotify::All)
            .await
            .unwrap(),
        event()
    );
    let mut patch = EnginePatch::default();
    patch.title = Some("Renamed".into());
    let reference = EngineRef::new("c1", "e1");
    adapter
        .update(
            &reference,
            &patch,
            EngineScope::ThisInstance,
            EngineNotify::None,
        )
        .await
        .unwrap();
    adapter
        .delete(
            &reference,
            EngineScope::AllInSeries,
            EngineNotify::ExternalOnly,
        )
        .await
        .unwrap();
    adapter
        .respond(
            &reference,
            EngineResponse::Declined,
            EngineScope::ThisAndFollowing,
            EngineNotify::All,
        )
        .await
        .unwrap();
    assert_eq!(
        *fake.order.lock().unwrap(),
        vec![
            "series s1 c1",
            "create New c1 All",
            "update e1 Some(\"Renamed\") ThisInstance None",
            "delete e1 AllInSeries ExternalOnly",
            "respond e1 Declined ThisAndFollowing All",
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_system_hosts_typed_errors_reach_the_engine_unchanged() {
    let fake = FakeSystem::new();
    let adapter = SystemCalendarAdapter::new(fake.clone());
    *fake.error.lock().unwrap() = Some(FfiError::Source(SourceError::NeedsPermission));
    assert_eq!(
        adapter.calendars().await.unwrap_err(),
        Failure::Source(EngineSourceError::NeedsPermission)
    );
    *fake.error.lock().unwrap() = Some(FfiError::Write(
        crate::values::write::WriteError::Forbidden(Some("read only".into())),
    ));
    let reference = EngineRef::new("c", "e");
    assert_eq!(
        adapter
            .delete(&reference, EngineScope::ThisInstance, EngineNotify::All)
            .await
            .unwrap_err(),
        Failure::Write(EngineWriteError::Forbidden(Some("read only".into())))
    );
    *fake.error.lock().unwrap() = Some(FfiError::Cancelled);
    assert_eq!(
        adapter
            .events_in(DateInterval::new(at(1), at(2)))
            .await
            .unwrap_err(),
        Failure::Cancelled
    );
}

#[tokio::test(flavor = "current_thread")]
async fn three_handler_calls_are_three_changes_in_order() {
    let fake = FakeSystem::new();
    let adapter = SystemCalendarAdapter::new(fake.clone());
    let mut changes = adapter.changes();
    adapter.calendars().await.unwrap();
    let handler = fake.handler.lock().unwrap().clone().unwrap();
    handler.events_changed(Some(vec!["a".into(), "b".into()]));
    handler.calendars_changed();
    handler.source_failed(SourceError::AuthExpired);
    assert_eq!(
        changes.recv().await,
        Some(CalendarChange::EventsChanged {
            calendar_ids: Some(["a".to_string(), "b".to_string()].into())
        })
    );
    assert_eq!(changes.recv().await, Some(CalendarChange::CalendarsChanged));
    assert_eq!(
        changes.recv().await,
        Some(CalendarChange::SourceFailed(EngineSourceError::AuthExpired))
    );
    // A second subscriber sees only later changes; a dropped one is pruned.
    let second = adapter.changes();
    drop(second);
    handler.events_changed(None);
    assert_eq!(
        changes.recv().await,
        Some(CalendarChange::EventsChanged { calendar_ids: None })
    );
}
