//! The `Engine` object with Rust fakes for every host interface: construction (each invalid configuration), the first
//! observer call, a command that changes state, a full iCal-link scenario on the manual clock (account, events, takeover),
//! and the shutdown rules (observer silence, shutdown from inside an observer call, drop without shutdown).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use jiff::Timestamp;

use crate::config::{EngineConfig, GoogleClient};
use crate::engine::{ClockChoice, Engine, EngineHostInterfaces};
use crate::errors::{FfiError, HostError};
use crate::host::authorization::{AuthorizationInteraction, OAuthRedirectHandle};
use crate::host::credentials::CredentialStore;
use crate::host::http::HttpTransport;
use crate::host::observer::EngineObserver;
use crate::host::settings_storage::SettingsStorage;
use crate::host::sign_in::SignInPrompting;
use crate::values::ToMirror;
use crate::values::accounts::{CredentialField, CredentialHelp, CredentialSnapshot, Platform};
use crate::values::commands::{EngineCommand, SystemEvent};
use crate::values::events::EngineEvent;
use crate::values::host::{HttpRequest, HttpResponse};
use crate::values::settings::{EngineSettings, SettingsPatch};
use crate::values::summary::EngineStateSummary;

const CLOCK_START: i64 = 1_794_650_400; // 2026-11-14T10:00:00Z

fn clock_start() -> Timestamp {
    Timestamp::from_second(CLOCK_START).unwrap()
}

const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:uid-1@example.com\r\nDTSTAMP:20261114T100000Z\r\nDTSTART:20261114T110000Z\r\nDTEND:20261114T113000Z\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

// ---------- fakes ----------

#[derive(Default)]
struct MemoryCredentials(Mutex<HashMap<String, HashMap<String, String>>>);

#[async_trait]
impl CredentialStore for MemoryCredentials {
    async fn secrets(
        &self,
        connection_id: String,
    ) -> Result<Option<HashMap<String, String>>, HostError> {
        Ok(self.0.lock().unwrap().get(&connection_id).cloned())
    }
    async fn set_secrets(
        &self,
        secrets: HashMap<String, String>,
        connection_id: String,
    ) -> Result<(), HostError> {
        self.0.lock().unwrap().insert(connection_id, secrets);
        Ok(())
    }
    async fn remove_secrets(&self, connection_id: String) -> Result<(), HostError> {
        self.0.lock().unwrap().remove(&connection_id);
        Ok(())
    }
    async fn credential_snapshot(
        &self,
        connection_id: String,
    ) -> Result<Option<CredentialSnapshot>, HostError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&connection_id)
            .map(|secrets| CredentialSnapshot {
                secrets: secrets.clone(),
                revision: uuid::Uuid::from_u128(1),
            }))
    }
    async fn update_refresh_token(
        &self,
        _token: String,
        _connection_id: String,
        _expected: uuid::Uuid,
    ) -> Result<bool, HostError> {
        Ok(false)
    }
}

/// A form that types the feed address.
struct LinkInteraction;

#[async_trait]
impl AuthorizationInteraction for LinkInteraction {
    async fn begin_oauth_redirect(&self) -> Result<OAuthRedirectHandle, HostError> {
        Err(HostError::Denied)
    }
    async fn prompt_credentials(
        &self,
        _fields: Vec<CredentialField>,
    ) -> Result<HashMap<String, String>, HostError> {
        Ok(HashMap::from([(
            "link".to_string(),
            "https://feeds.example.com/team.ics".to_string(),
        )]))
    }
}

struct NoPrompt;

#[async_trait]
impl SignInPrompting for NoPrompt {
    async fn prepare(
        &self,
        _title: String,
        _help: Option<CredentialHelp>,
        _values: HashMap<String, String>,
        _error: Option<String>,
    ) {
    }
    async fn last_non_secret_values(&self) -> Option<HashMap<String, String>> {
        None
    }
}

#[derive(Default)]
struct MemorySettings(Mutex<Option<EngineSettings>>);

impl SettingsStorage for MemorySettings {
    fn load(&self) -> EngineSettings {
        self.0
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| timetug_engine::settings::EngineSettings::default().to_mirror())
    }
    fn save(&self, settings: EngineSettings) {
        *self.0.lock().unwrap() = Some(settings);
    }
}

/// Serves the feed for any address.
struct FeedHttp;

#[async_trait]
impl HttpTransport for FeedHttp {
    async fn send(&self, _request: HttpRequest) -> Result<HttpResponse, HostError> {
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::from([("content-type".to_string(), "text/calendar".to_string())]),
            body: FEED.as_bytes().to_vec(),
        })
    }
}

type Hook = Arc<dyn Fn(&Observed) + Send + Sync>;

/// Records what the observer saw.
#[derive(Default)]
struct Observed {
    summaries: Mutex<Vec<EngineStateSummary>>,
    events: Mutex<Vec<EngineEvent>>,
    calls: AtomicUsize,
    first_is_state: Mutex<Option<bool>>,
    hook: Mutex<Option<Hook>>,
}

impl Observed {
    fn summaries(&self) -> Vec<EngineStateSummary> {
        self.summaries.lock().unwrap().clone()
    }
    fn events(&self) -> Vec<EngineEvent> {
        self.events.lock().unwrap().clone()
    }
    fn note_first(&self, is_state: bool) {
        self.first_is_state.lock().unwrap().get_or_insert(is_state);
    }
    fn run_hook(&self) {
        let hook = self.hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(self);
        }
    }
}

struct Recorder(Arc<Observed>);

impl EngineObserver for Recorder {
    fn state_changed(&self, summary: EngineStateSummary) {
        self.0.note_first(true);
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        self.0.summaries.lock().unwrap().push(summary);
        self.0.run_hook();
    }
    fn event(&self, event: EngineEvent) {
        self.0.note_first(false);
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        self.0.events.lock().unwrap().push(event);
        self.0.run_hook();
    }
}

fn interfaces(settings: Arc<MemorySettings>) -> EngineHostInterfaces {
    EngineHostInterfaces {
        credentials: Arc::new(MemoryCredentials::default()),
        authorization: Arc::new(LinkInteraction),
        sign_in: Arc::new(NoPrompt),
        settings,
        http: Arc::new(FeedHttp),
        diagnostics: None,
        text_generator: None,
        external_tokens: None,
        system_calendar: None,
    }
}

fn config(directory: &std::path::Path) -> EngineConfig {
    EngineConfig {
        data_directory: directory.to_string_lossy().into_owned(),
        platform: Platform::Macos,
        google_client: None,
        microsoft_client: None,
        text_generation: false,
        external_tokens: false,
        system_calendar: false,
        time_zone: "Europe/Berlin".into(),
        ical_days_back: 3,
        ical_days_ahead: 7,
    }
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "timetug-ffi-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    engine: Arc<Engine>,
    observed: Arc<Observed>,
    settings: Arc<MemorySettings>,
    _dir: TempDir,
}

fn fixture(name: &str) -> Fixture {
    let dir = TempDir::new(name);
    let observed = Arc::new(Observed::default());
    let settings = Arc::new(MemorySettings::default());
    let engine = Engine::build(
        config(&dir.0),
        interfaces(settings.clone()),
        Arc::new(Recorder(observed.clone())),
        ClockChoice::Manual(clock_start()),
    )
    .unwrap();
    Fixture {
        engine,
        observed,
        settings,
        _dir: dir,
    }
}

/// Drives an exported async method of the engine from a test thread.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---------- construction ----------

fn invalid_reason(result: Result<Arc<Engine>, FfiError>) -> String {
    match result {
        Err(FfiError::InvalidConfig { reason }) => reason,
        Err(other) => panic!("not InvalidConfig: {other:?}"),
        Ok(_) => panic!("the engine was created"),
    }
}

#[test]
fn every_invalid_configuration_is_refused_with_its_reason() {
    let dir = TempDir::new("invalid");
    let build = |config: EngineConfig, host: EngineHostInterfaces| {
        Engine::build(
            config,
            host,
            Arc::new(Recorder(Arc::new(Observed::default()))),
            ClockChoice::System,
        )
    };
    let host = || interfaces(Arc::new(MemorySettings::default()));

    let mut bad = config(&dir.0);
    bad.data_directory = String::new();
    assert_eq!(
        invalid_reason(build(bad, host())),
        "the data directory is empty"
    );
    let mut bad = config(&dir.0);
    bad.time_zone = "Nowhere/Land".into();
    assert_eq!(
        invalid_reason(build(bad, host())),
        "unknown time zone identifier \"Nowhere/Land\""
    );
    let mut bad = config(&dir.0);
    bad.google_client = Some(GoogleClient {
        id: "g".into(),
        secret: String::new(),
    });
    assert_eq!(
        invalid_reason(build(bad, host())),
        "the Google client needs an id and a secret"
    );
    let mut bad = config(&dir.0);
    bad.text_generation = true;
    assert_eq!(
        invalid_reason(build(bad, host())),
        "textGeneration is on but no TextGenerator was passed"
    );
    let mut bad = config(&dir.0);
    bad.external_tokens = true;
    assert_eq!(
        invalid_reason(build(bad, host())),
        "externalTokens is on but no ExternalTokenSource was passed"
    );
    let mut bad = config(&dir.0);
    bad.system_calendar = true;
    assert_eq!(
        invalid_reason(build(bad, host())),
        "systemCalendar is on but no SystemCalendarSource was passed"
    );
    // A path below a file cannot become a directory.
    let file = dir.0.join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let mut bad = config(&dir.0);
    bad.data_directory = file.join("below").to_string_lossy().into_owned();
    assert!(invalid_reason(build(bad, host())).starts_with("cannot create the data directory: "));
}

// ---------- state and commands ----------

#[test]
fn the_first_observer_call_after_start_is_a_state() {
    let fixture = fixture("first-call");
    assert_eq!(
        fixture.observed.calls.load(Ordering::SeqCst),
        0,
        "nothing is delivered before start"
    );
    block_on(fixture.engine.start());
    wait_until("a state", || !fixture.observed.summaries().is_empty());
    assert_eq!(*fixture.observed.first_is_state.lock().unwrap(), Some(true));
    wait_until("the state of the start", || {
        fixture
            .observed
            .summaries()
            .last()
            .is_some_and(|s| s.revision >= 1)
    });
    let last = fixture.observed.summaries().last().unwrap().clone();
    assert!(fixture.engine.current_summary().revision >= last.revision);
    assert!(fixture.engine.current_events().is_empty());
    fixture.engine.shutdown();
}

#[test]
fn a_command_changes_the_state_and_reaches_the_hosts_storage() {
    let fixture = fixture("settings");
    block_on(fixture.engine.start());
    let patch = SettingsPatch {
        lead_time: Some(120.0),
        ..SettingsPatch::default()
    };
    block_on(fixture.engine.send(EngineCommand::ApplySettings(patch)));
    wait_until("the new lead time", || {
        fixture
            .observed
            .summaries()
            .last()
            .is_some_and(|s| s.settings.takeover.lead_time == 120.0)
    });
    assert_eq!(
        fixture.engine.current_summary().settings.takeover.lead_time,
        120.0
    );
    assert_eq!(
        fixture
            .settings
            .0
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .takeover
            .lead_time,
        120.0
    );
    fixture.engine.shutdown();
}

#[test]
fn a_link_account_shows_its_events_and_a_takeover_comes_due_when_the_clock_gets_there() {
    let fixture = fixture("scenario");
    block_on(fixture.engine.start());
    block_on(fixture.engine.send(EngineCommand::AddAccount {
        kind_id: "icalsub".into(),
    }));
    wait_until("the account and its event", || {
        fixture
            .observed
            .summaries()
            .last()
            .is_some_and(|s| s.accounts.len() == 1)
            && !fixture.engine.current_events().is_empty()
    });
    let summary = fixture.engine.current_summary();
    assert_eq!(summary.accounts[0].kind_id, "icalsub");
    let events = fixture.engine.current_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.title, "Standup");
    assert_eq!(events[0].event.start.as_second(), CLOCK_START + 3600);
    assert_eq!(
        summary.events_revision,
        summary.revision.min(summary.events_revision)
    );
    let events_revision = summary.events_revision;
    assert!(events_revision > 0, "the events changed in some state");
    assert_eq!(fixture.engine.current_agenda().items.len(), 1);

    // Opt the calendar in, then let the clock reach the lead time.
    let calendar = &summary.calendars[0];
    let key = format!("{}/{}", calendar.source_id, calendar.calendar_id);
    let patch = SettingsPatch {
        takeover_calendar_keys: Some([key].into()),
        ..SettingsPatch::default()
    };
    block_on(fixture.engine.send(EngineCommand::ApplySettings(patch)));
    wait_until("the takeover to be scheduled", || {
        fixture.engine.current_summary().next_takeover.is_some()
    });
    // A state change that does not touch the events keeps the events revision.
    assert_eq!(
        fixture.engine.current_summary().events_revision,
        events_revision
    );
    let fire_at = fixture
        .engine
        .current_summary()
        .next_takeover
        .unwrap()
        .fire_at
        .as_second();
    assert_eq!(fire_at, CLOCK_START + 3600 - 60);
    fixture.engine.advance_manual_clock(3600.0 - 30.0);
    wait_until("takeoverDue", || {
        fixture
            .observed
            .events()
            .iter()
            .any(|e| matches!(e, EngineEvent::TakeoverDue { .. }))
    });
    let due = fixture
        .observed
        .events()
        .into_iter()
        .find(|e| matches!(e, EngineEvent::TakeoverDue { .. }))
        .unwrap();
    let EngineEvent::TakeoverDue { request, .. } = due else {
        panic!("not due")
    };
    assert_eq!(request.event.event.title, "Standup");
    block_on(fixture.engine.send(EngineCommand::Dismiss {
        event_id: request.event.event.event_id.clone(),
    }));
    block_on(
        fixture
            .engine
            .send(EngineCommand::System(SystemEvent::DisplayChanged)),
    );
    fixture.engine.shutdown();
}

#[test]
fn an_unknown_time_zone_is_refused_and_the_zone_is_unchanged() {
    let fixture = fixture("zone");
    assert!(fixture.engine.set_time_zone("Asia/Tokyo".into()).is_ok());
    assert_eq!(
        fixture.engine.set_time_zone("Mars/Base".into()),
        Err(FfiError::InvalidConfig {
            reason: "unknown time zone identifier \"Mars/Base\"".into()
        })
    );
    fixture.engine.shutdown();
}

// ---------- shutdown ----------

#[test]
fn shutdown_stops_observer_calls_and_is_idempotent() {
    let fixture = fixture("shutdown");
    block_on(fixture.engine.start());
    block_on(fixture.engine.send(EngineCommand::AddAccount {
        kind_id: "icalsub".into(),
    }));
    wait_until("the account", || {
        fixture
            .observed
            .summaries()
            .last()
            .is_some_and(|s| s.accounts.len() == 1)
    });
    fixture.engine.shutdown();
    let calls = fixture.observed.calls.load(Ordering::SeqCst);
    fixture.engine.shutdown();
    fixture.engine.advance_manual_clock(86_400.0);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        fixture.observed.calls.load(Ordering::SeqCst),
        calls,
        "no observer call after shutdown returned"
    );
    // A command after shutdown returns at once instead of waiting for an engine that is gone.
    let started = Instant::now();
    block_on(fixture.engine.send(EngineCommand::Refresh));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn shutdown_inside_an_observer_call_returns_at_once_and_no_further_call_starts() {
    let fixture = fixture("shutdown-inside");
    let engine = fixture.engine.clone();
    let returned_in = Arc::new(Mutex::new(None));
    let slot = returned_in.clone();
    *fixture.observed.hook.lock().unwrap() = Some(Arc::new(move |_| {
        let begun = Instant::now();
        engine.shutdown();
        slot.lock().unwrap().get_or_insert(begun.elapsed());
    }));
    block_on(fixture.engine.start());
    wait_until("the first call", || {
        fixture.observed.calls.load(Ordering::SeqCst) >= 1
    });
    wait_until("shutdown to have run inside the call", || {
        returned_in.lock().unwrap().is_some()
    });
    assert!(returned_in.lock().unwrap().unwrap() < Duration::from_secs(1));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(fixture.observed.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn dropping_the_engine_without_shutdown_stops_it() {
    let dir = TempDir::new("drop");
    let observed = Arc::new(Observed::default());
    let engine = Engine::build(
        config(&dir.0),
        interfaces(Arc::new(MemorySettings::default())),
        Arc::new(Recorder(observed.clone())),
        ClockChoice::Manual(clock_start()),
    )
    .unwrap();
    block_on(engine.start());
    wait_until("a state", || observed.calls.load(Ordering::SeqCst) >= 1);
    let started = Instant::now();
    drop(engine);
    assert!(started.elapsed() < Duration::from_secs(5));
    let calls = observed.calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(observed.calls.load(Ordering::SeqCst), calls);
}
