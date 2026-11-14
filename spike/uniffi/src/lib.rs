//! Throwaway spike: what UniFFI 0.32.2 can express for the TimeTug engine bindings.
#![allow(missing_docs)]
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

// ---------- custom types (local newtypes; remote jiff/uuid types need newtypes anyway) ----------
pub struct Timestamp(pub i64);
uniffi::custom_newtype!(Timestamp, i64);
pub struct UrlString(pub String);
uniffi::custom_newtype!(UrlString, String);
pub struct Uid(pub String);
uniffi::custom_newtype!(Uid, String);
#[derive(Clone, Debug, PartialEq)]
pub struct Zone(pub String);
uniffi::custom_type!(Zone, String, {
    lower: |z| z.0,
    try_lift: |s| if s == "Asia/Tokyo" { Err(SpikeError::InvalidConfig { reason: s }.into()) } else { Ok(Zone(s)) },
});
impl Clone for Timestamp { fn clone(&self) -> Self { Timestamp(self.0) } }
impl std::fmt::Debug for Timestamp { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "T{}", self.0) } }
impl PartialEq for Timestamp { fn eq(&self, o: &Self) -> bool { self.0 == o.0 } }
impl Clone for UrlString { fn clone(&self) -> Self { UrlString(self.0.clone()) } }
impl std::fmt::Debug for UrlString { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) } }
impl PartialEq for UrlString { fn eq(&self, o: &Self) -> bool { self.0 == o.0 } }
impl Clone for Uid { fn clone(&self) -> Self { Uid(self.0.clone()) } }
impl std::fmt::Debug for Uid { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) } }
impl PartialEq for Uid { fn eq(&self, o: &Self) -> bool { self.0 == o.0 } }

// ---------- errors ----------
#[derive(Debug, thiserror::Error, uniffi::Error, Clone, PartialEq)]
pub enum SpikeError {
    #[error("source: {0}")]
    Source(SourceErrorKind),
    #[error("cancelled")]
    Cancelled,
    #[error("host: {message}")]
    Host { message: String },
    #[error("invalid config: {reason}")]
    InvalidConfig { reason: String },
}
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SourceErrorKind { AuthExpired, Network { message: String }, Http { status: u16 } }
impl std::fmt::Display for SourceErrorKind { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") } }

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HostError {
    #[error("network {0}")]
    Network(String),
    #[error("cancelled")]
    Cancelled,
    #[error("not found")]
    NotFound,
    #[error("denied")]
    Denied,
    #[error("failed {0}")]
    Failed(String),
}
impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self { HostError::Failed(e.reason) }
}
impl From<HostError> for SpikeError {
    fn from(e: HostError) -> Self {
        match e {
            HostError::Cancelled => SpikeError::Cancelled,
            other => SpikeError::Host { message: other.to_string() },
        }
    }
}

// ---------- enums and records ----------
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SeriesInfo { None, Master, Occurrence { series_id: String, original_start: Timestamp } }
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Participation { None, Attending { comment: Option<String> }, Declined(String), Tentative(f64, String) }
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ReminderTrigger { Relative(f64), Absolute(Timestamp) }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ReminderKind { Alert, Email }
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Reminder { pub trigger: ReminderTrigger, pub kind: ReminderKind }
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Attendee { pub name: Option<String>, #[uniffi(default = false)] pub is_self: bool, pub email: String, pub response: Participation }
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Conference { pub url: UrlString, pub provider: String }
/// A name that clashes with Foundation.Calendar.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Calendar { pub id: String, pub title: String }
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Connection { pub id: Uid, pub label: String }
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum FieldUpdateString { Keep, Clear, Set(String) }
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EventEdit { Title(String), Time { start: Timestamp, end: Timestamp }, Notes(FieldUpdateString), Many(Vec<EventEdit>) }

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BigEvent {
    pub f00_x: String,
    pub f01_x: Timestamp,
    pub f02_x: Option<Timestamp>,
    pub f03_x: bool,
    pub f04_x: f64,
    pub f05_x: i64,
    pub f06_x: Option<String>,
    pub f07_x: Vec<String>,
    pub f08_x: Vec<Attendee>,
    pub f09_x: Option<Conference>,
    pub f10_x: HashMap<String,String>,
    pub f11_x: HashSet<String>,
    pub f12_x: SeriesInfo,
    pub f13_x: Vec<Reminder>,
    pub f14_x: Option<Zone>,
    pub f15_x: Option<UrlString>,
    pub f16_x: Uid,
    pub f17_x: Vec<u8>,
    pub f18_x: Participation,
    pub f19_x: u32,
    pub f20_x: String,
    pub f21_x: Timestamp,
    pub f22_x: Option<Timestamp>,
    pub f23_x: bool,
    pub f24_x: f64,
    pub f25_x: i64,
    pub f26_x: Option<String>,
    pub f27_x: Vec<String>,
    pub f28_x: Vec<Attendee>,
    pub f29_x: Option<Conference>,
    pub f30_x: HashMap<String,String>,
    pub f31_x: HashSet<String>,
    pub f32_x: SeriesInfo,
    pub f33_x: Vec<Reminder>,
    pub f34_x: Option<Zone>,
    pub f35_x: Option<UrlString>,
    pub f36_x: Uid,
    pub f37_x: Vec<u8>,
    pub f38_x: Participation,
    pub f39_x: u32,
}
fn attendee(seed: u32) -> Attendee {
    Attendee { name: Some(format!("A{seed}")), is_self: seed % 3 == 0, email: format!("a{seed}@example.com"), response: Participation::Tentative(0.5, "x".into()) }
}
fn make_event(seed: u32) -> BigEvent {
    BigEvent {
        f00_x: format!("s{}-{}", seed, 0),
        f01_x: Timestamp(1_700_000_000_000 + seed as i64),
        f02_x: Some(Timestamp(1_700_000_100_000)),
        f03_x: seed % 2 == 0,
        f04_x: seed as f64 * 0.5,
        f05_x: seed as i64,
        f06_x: Some("note \u{1F600} \u{0301}".to_string()),
        f07_x: vec!["a".to_string(), "b".to_string()],
        f08_x: vec![attendee(seed), attendee(seed + 1)],
        f09_x: Some(Conference { url: UrlString("https://meet.example/x".into()), provider: "meet".into() }),
        f10_x: HashMap::from([("k".to_string(), "v".to_string())]),
        f11_x: HashSet::from(["z".to_string()]),
        f12_x: SeriesInfo::Occurrence { series_id: "s".into(), original_start: Timestamp(5) },
        f13_x: vec![Reminder { trigger: ReminderTrigger::Relative(-300.0), kind: ReminderKind::Alert }, Reminder { trigger: ReminderTrigger::Absolute(Timestamp(9)), kind: ReminderKind::Email }],
        f14_x: Some(Zone("Europe/Berlin".into())),
        f15_x: Some(UrlString("https://example.com/\u{e9}".into())),
        f16_x: Uid("00000000-0000-0000-0000-000000000001".into()),
        f17_x: vec![0, 1, 2, 255],
        f18_x: Participation::Attending { comment: None },
        f19_x: seed,
        f20_x: format!("s{}-{}", seed, 20),
        f21_x: Timestamp(1_700_000_000_000 + seed as i64),
        f22_x: Some(Timestamp(1_700_000_100_000)),
        f23_x: seed % 2 == 0,
        f24_x: seed as f64 * 0.5,
        f25_x: seed as i64,
        f26_x: Some("note \u{1F600} \u{0301}".to_string()),
        f27_x: vec!["a".to_string(), "b".to_string()],
        f28_x: vec![attendee(seed), attendee(seed + 1)],
        f29_x: Some(Conference { url: UrlString("https://meet.example/x".into()), provider: "meet".into() }),
        f30_x: HashMap::from([("k".to_string(), "v".to_string())]),
        f31_x: HashSet::from(["z".to_string()]),
        f32_x: SeriesInfo::Occurrence { series_id: "s".into(), original_start: Timestamp(5) },
        f33_x: vec![Reminder { trigger: ReminderTrigger::Relative(-300.0), kind: ReminderKind::Alert }, Reminder { trigger: ReminderTrigger::Absolute(Timestamp(9)), kind: ReminderKind::Email }],
        f34_x: Some(Zone("Europe/Berlin".into())),
        f35_x: Some(UrlString("https://example.com/\u{e9}".into())),
        f36_x: Uid("00000000-0000-0000-0000-000000000001".into()),
        f37_x: vec![0, 1, 2, 255],
        f38_x: Participation::Attending { comment: None },
        f39_x: seed,
    }
}
#[uniffi::export]
pub fn make_agenda(count: u32) -> Vec<BigEvent> { (0..count).map(make_event).collect() }
#[uniffi::export]
pub fn echo_event(e: BigEvent) -> BigEvent { e }
#[uniffi::export]
pub fn echo_events(e: Vec<BigEvent>) -> Vec<BigEvent> { e }
#[uniffi::export]
pub fn echo_edit(e: EventEdit) -> EventEdit { e }
#[uniffi::export]
pub fn echo_string(s: String) -> String { s }
#[uniffi::export]
pub fn echo_timestamp(t: Timestamp) -> Timestamp { t }
#[uniffi::export]
pub fn echo_zone(z: Zone) -> Zone { z }
#[uniffi::export]
pub fn echo_zone_throwing(z: Zone) -> Result<Zone, SpikeError> { Ok(z) }
#[uniffi::export]
pub fn echo_url(u: UrlString) -> UrlString { u }
#[uniffi::export]
pub fn echo_uuid(u: Uid) -> Uid { u }
#[uniffi::export]
pub fn echo_data(d: Vec<u8>) -> Vec<u8> { d }
#[uniffi::export]
pub fn echo_duration(d: std::time::Duration) -> std::time::Duration { d }
#[uniffi::export]
pub fn echo_calendar(c: Calendar) -> Calendar { c }
#[uniffi::export]
pub fn echo_connection(c: Connection) -> Connection { c }
#[uniffi::export]
pub fn fail_with(kind: u8) -> Result<(), SpikeError> {
    match kind {
        0 => Err(SpikeError::Source(SourceErrorKind::AuthExpired)),
        1 => Err(SpikeError::Source(SourceErrorKind::Http { status: 503 })),
        2 => Err(SpikeError::Cancelled),
        _ => Err(SpikeError::Host { message: "boom".into() }),
    }
}

// ---------- foreign traits ----------
#[uniffi::export(foreign)]
#[async_trait::async_trait]
pub trait RedirectSession: Send + Sync {
    async fn redirect_uri(&self) -> Result<UrlString, HostError>;
    async fn close(&self);
}
#[derive(uniffi::Record)]
pub struct RedirectBegun { pub session: Arc<dyn RedirectSession> }
#[uniffi::export(foreign)]
#[async_trait::async_trait]
pub trait AuthInteraction: Send + Sync {
    async fn begin_redirect(&self) -> Result<RedirectBegun, HostError>;
    fn begin_redirect_sync(&self) -> Result<Arc<dyn RedirectSession>, HostError>;
}
#[uniffi::export(foreign)]
#[async_trait::async_trait]
pub trait CredentialStore: Send + Sync {
    async fn secrets(&self, id: Uid) -> Result<Option<Vec<u8>>, HostError>;
    async fn set_secrets(&self, id: Uid, value: Vec<u8>) -> Result<(), HostError>;
    async fn never_returns(&self) -> Result<(), HostError>;
    async fn on_main_actor(&self) -> Result<String, HostError>;
}
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SysCalendar { pub id: String, pub title: String, pub writable: bool }
#[uniffi::export(foreign)]
#[async_trait::async_trait]
pub trait SystemSource: Send + Sync {
    async fn request_access(&self) -> Result<bool, HostError>;
    async fn calendars(&self) -> Result<Vec<SysCalendar>, HostError>;
    async fn events_in(&self, start: Timestamp, end: Timestamp) -> Result<Vec<BigEvent>, HostError>;
    async fn series(&self, id: String, calendar_id: String) -> Result<Option<BigEvent>, HostError>;
    async fn capabilities(&self) -> Result<Option<SysCalendar>, HostError>;
    async fn create_event(&self, draft: BigEvent) -> Result<BigEvent, HostError>;
    async fn update_event(&self, id: String, edit: EventEdit) -> Result<Option<BigEvent>, HostError>;
    async fn delete_event(&self, id: String) -> Result<(), HostError>;
    async fn move_event(&self, id: String, to: String) -> Result<Option<BigEvent>, HostError>;
    async fn rsvp(&self, id: String, p: Participation) -> Result<(), HostError>;
    async fn set_change_handler(&self, handler: Arc<ChangeHandler>) -> Result<(), HostError>;
    async fn refresh(&self) -> Result<(), HostError>;
}
#[derive(uniffi::Object, Default)]
pub struct ChangeHandler { count: AtomicU64 }
#[uniffi::export]
impl ChangeHandler {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }
    pub fn changed(&self) { self.count.fetch_add(1, Ordering::SeqCst); }
    pub fn count(&self) -> u64 { self.count.load(Ordering::SeqCst) }
}
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SpikeCommand { Refresh, Dismiss { id: String }, Settings(EventEdit) }
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SpikeState { pub revision: u64, pub events: Vec<BigEvent> }
#[uniffi::export(foreign)]
pub trait Observer: Send + Sync {
    fn state_changed(&self, state: SpikeState);
    fn event(&self, name: String);
}
/// A foreign trait inside a record.
#[derive(uniffi::Record)]
pub struct HostBundle {
    pub store: Arc<dyn CredentialStore>,
    pub system: Option<Arc<dyn SystemSource>>,
    pub auth: Arc<dyn AuthInteraction>,
}

// ---------- the engine object ----------
#[derive(uniffi::Object)]
pub struct SpikeEngine {
    handle: tokio::runtime::Handle,
    stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    host: HostBundle,
    observer: Arc<dyn Observer>,
    revision: AtomicU64,
}
#[uniffi::export]
impl SpikeEngine {
    #[uniffi::constructor]
    pub fn new(host: HostBundle, observer: Arc<dyn Observer>) -> Result<Arc<Self>, SpikeError> {
        let (htx, hrx) = std::sync::mpsc::channel();
        let (stx, srx) = tokio::sync::oneshot::channel::<()>();
        std::thread::Builder::new().name("engine".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_time().build() {
                Ok(rt) => rt,
                Err(_) => return,
            };
            let _ = htx.send(rt.handle().clone());
            rt.block_on(async move { let _ = srx.await; });
        }).map_err(|e| SpikeError::InvalidConfig { reason: e.to_string() })?;
        let handle = hrx.recv().map_err(|e| SpikeError::InvalidConfig { reason: e.to_string() })?;
        Ok(Arc::new(Self { handle, stop: Mutex::new(Some(stx)), host, observer, revision: AtomicU64::new(0) }))
    }
    /// Runs on the engine runtime; the foreign executor only awaits a oneshot.
    pub async fn send(&self, command: SpikeCommand) -> Result<String, SpikeError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let store = self.host.store.clone();
        let auth = self.host.auth.clone();
        let system = self.host.system.clone();
        let observer = self.observer.clone();
        let rev = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.handle.spawn(async move {
            let result: Result<String, SpikeError> = async {
                match command {
                    SpikeCommand::Refresh => {
                        let got = store.secrets(Uid("00000000-0000-0000-0000-000000000001".into())).await?;
                        let session = auth.begin_redirect().await?.session;
                        let uri = session.redirect_uri().await?;
                        session.close().await;
                        if let Some(sys) = system {
                            let cals = sys.calendars().await?;
                            let _ = sys.events_in(Timestamp(0), Timestamp(1)).await?;
                            observer.event(format!("calendars={}", cals.len()));
                        }
                        observer.state_changed(SpikeState { revision: rev, events: make_agenda(3) });
                        Ok(format!("{:?} {}", got, uri.0))
                    }
                    SpikeCommand::Dismiss { id } => Ok(id),
                    SpikeCommand::Settings(_) => Ok("settings".into()),
                }
            }.await;
            let _ = tx.send(result);
        });
        rx.await.map_err(|_| SpikeError::Cancelled)?
    }
    /// Calls the never-returning host method with a timeout on the engine runtime, then drops the future.
    pub async fn call_never_with_timeout(&self, ms: u64) -> String {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let store = self.host.store.clone();
        self.handle.spawn(async move {
            let r = tokio::time::timeout(std::time::Duration::from_millis(ms), store.never_returns()).await;
            let _ = tx.send(if r.is_err() { "timed out, future dropped".to_string() } else { "returned".to_string() });
        });
        rx.await.unwrap_or_else(|_| "cancelled".into())
    }
    pub async fn call_main_actor(&self) -> Result<String, SpikeError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let store = self.host.store.clone();
        self.handle.spawn(async move { let _ = tx.send(store.on_main_actor().await); });
        Ok(rx.await.map_err(|_| SpikeError::Cancelled)??)
    }
    /// A system source with 12 methods driven through one call.
    pub async fn exercise_system(&self) -> Result<u32, SpikeError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let sys = self.host.system.clone();
        self.handle.spawn(async move {
            let r: Result<u32, SpikeError> = async {
                let Some(s) = sys else { return Ok(0) };
                let mut n = 0;
                if s.request_access().await? { n += 1 }
                n += s.calendars().await?.len() as u32;
                n += s.events_in(Timestamp(0), Timestamp(10)).await?.len() as u32;
                if s.series("a".into(), "b".into()).await?.is_some() { n += 100 }
                if s.capabilities().await?.is_some() { n += 1000 }
                let e = s.create_event(make_event(1)).await?;
                if s.update_event(e.f00_x.clone(), EventEdit::Title("t".into())).await?.is_some() { n += 10000 }
                s.delete_event("x".into()).await?;
                if s.move_event("x".into(), "y".into()).await?.is_some() { n += 100000 }
                s.rsvp("x".into(), Participation::None).await?;
                s.set_change_handler(ChangeHandler::new()).await?;
                s.refresh().await?;
                Ok(n)
            }.await;
            let _ = tx.send(r);
        });
        rx.await.map_err(|_| SpikeError::Cancelled)?
    }
    /// Delivers `count` states from a delivery thread to the observer (rate and cost probe).
    pub fn blast_states(&self, count: u32, events: u32) {
        let observer = self.observer.clone();
        let template = make_agenda(events);
        std::thread::spawn(move || {
            for i in 0..count {
                observer.state_changed(SpikeState { revision: i as u64, events: template.clone() });
            }
            observer.event("blast-done".into());
        });
    }
    pub fn shutdown(&self) {
        if let Ok(mut g) = self.stop.lock() { if let Some(tx) = g.take() { let _ = tx.send(()); } }
    }
}
impl Drop for SpikeEngine { fn drop(&mut self) { self.shutdown(); } }

#[uniffi::export]
pub fn count_events(e: Vec<BigEvent>) -> u32 { e.len() as u32 }
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Summary { pub revision: u64, pub account_ok: bool, pub next_takeover: Option<Timestamp>, pub flags: u32 }
#[uniffi::export]
pub fn make_summary(r: u64) -> Summary { Summary { revision: r, account_ok: true, next_takeover: Some(Timestamp(1)), flags: 3 } }
#[uniffi::export]
pub fn encoded_size_probe(count: u32) -> u64 { // size of the lowered buffer cannot be read here; return the Debug text length as a proxy
    format!("{:?}", make_agenda(count)).len() as u64
}
