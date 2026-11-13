//! Port of `RequestTable.swift`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use calendar_core::failure::Failure;
use calendar_core::time::add_seconds;
use serde::de::DeserializeOwned;
use serde_json::Value;
use timetug_engine::clock::EngineClock;
use timetug_engine_protocol::envelope::{Envelope, MessageId};
use timetug_engine_protocol::service_messages::{ReplyErrorKind, WireReply};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// A host-service request that failed: the reply's error kind and message, or a timeout (`network`).
/// A `cancelled` reply is [`RequestError::Cancelled`] instead, so the engine treats it as a cancelled operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceError {
    /// The reply's error kind (`network` for a timeout).
    pub kind: ReplyErrorKind,
    /// The host's message; may be empty.
    pub message: String,
}

impl fmt::Display for ServiceError {
    /// Swift's `description`: the message, or the kind's raw value when there is none, otherwise `kind: message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            f.write_str(self.kind.as_str())
        } else {
            write!(f, "{}: {}", self.kind.as_str(), self.message)
        }
    }
}

/// Why a request did not produce a result: Swift's `CancellationError` or its `ServiceError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// The host answered `cancelled`, or the request was dropped or abandoned before an answer.
    Cancelled,
    /// The host answered with another error kind, or the request timed out.
    Service(ServiceError),
}

impl RequestError {
    fn from_kind(kind: ReplyErrorKind, message: &str) -> RequestError {
        if kind == ReplyErrorKind::Cancelled {
            RequestError::Cancelled
        } else {
            RequestError::Service(ServiceError {
                kind,
                message: message.to_string(),
            })
        }
    }
}

impl From<RequestError> for Failure {
    /// A `ServiceError` reaches the connector library as a host failure carrying its description; a cancellation stays
    /// a cancellation.
    fn from(error: RequestError) -> Failure {
        match error {
            RequestError::Cancelled => Failure::Cancelled,
            RequestError::Service(error) => Failure::Host(error.to_string()),
        }
    }
}

struct Pending {
    answer: oneshot::Sender<Result<Value, RequestError>>,
    timer: Option<JoinHandle<()>>,
}

struct State {
    last_id: u64,
    pending: BTreeMap<u64, Pending>,
    /// Set by `fail_all`: every later request fails at once with this kind and is never sent.
    closed_kind: Option<ReplyErrorKind>,
}

/// The engine's outstanding host-service requests. `request` sends one `service.*` envelope with a fresh id and
/// suspends until the host answers through `tt_engine_reply` (`fulfill`), the optional timeout passes on the engine
/// clock, the calling future is dropped (a cancelled task), or `fail_all` closes the table (destroy). Every path
/// resolves the request exactly once; a reply for an unknown or already resolved id is ignored.
///
/// `request` must be awaited inside the engine's `tokio` runtime: the timeout is a task on it.
pub struct RequestTable {
    send: Arc<dyn Fn(Envelope) + Send + Sync>,
    clock: Arc<dyn EngineClock>,
    weak: Weak<RequestTable>,
    state: Mutex<State>,
}

/// Forgets a request whose caller was dropped before the answer (Swift's `onCancel: cancel(id:)`).
struct CallerGuard<'a> {
    table: &'a RequestTable,
    id: u64,
}

impl Drop for CallerGuard<'_> {
    fn drop(&mut self) {
        self.table.cancel(self.id);
    }
}

impl RequestTable {
    /// Creates a table that sends each request envelope through `send` and times requests on `clock`.
    pub fn new(
        send: Arc<dyn Fn(Envelope) + Send + Sync>,
        clock: Arc<dyn EngineClock>,
    ) -> Arc<RequestTable> {
        Arc::new_cyclic(|weak| RequestTable {
            send,
            clock,
            weak: weak.clone(),
            state: Mutex::new(State {
                last_id: 0,
                pending: BTreeMap::new(),
                closed_kind: None,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The number of requests waiting for an answer.
    pub fn pending_count(&self) -> usize {
        self.lock().pending.len()
    }

    /// Sends one `message_type` envelope with `payload` and a fresh id and waits for the host's answer. With a
    /// `timeout` (seconds on the engine clock) it fails with a `network` error when no answer comes in time. Fails
    /// at once when the table is closed.
    pub async fn request(
        &self,
        message_type: &str,
        payload: Option<Value>,
        timeout: Option<f64>,
    ) -> Result<Value, RequestError> {
        // The deadline is fixed now, so a clock advanced before the timer task first runs still counts.
        let deadline = timeout.map(|seconds| add_seconds(self.clock.now(), seconds));
        let (answer, answered) = oneshot::channel();
        let id = {
            let mut state = self.lock();
            if let Some(kind) = state.closed_kind {
                return Err(RequestError::from_kind(kind, "the engine is shutting down"));
            }
            state.last_id = state.last_id.wrapping_add(1);
            let id = state.last_id;
            let timer = match (deadline, timeout) {
                (Some(deadline), Some(seconds)) => Some(self.start_timer(id, deadline, seconds)),
                _ => None,
            };
            state.pending.insert(id, Pending { answer, timer });
            id
        };
        let _caller = CallerGuard { table: self, id };
        let mut envelope = Envelope::new(message_type).with_id(MessageId::Number(id));
        envelope.payload = payload;
        (self.send)(envelope);
        // The sender is dropped without an answer only when the entry is removed by `cancel`, which is this future's
        // own guard.
        answered.await.unwrap_or(Err(RequestError::Cancelled))
    }

    fn start_timer(&self, id: u64, deadline: jiff::Timestamp, seconds: f64) -> JoinHandle<()> {
        let clock = self.clock.clone();
        let weak = self.weak.clone();
        tokio::spawn(async move {
            if clock.sleep_until(deadline).await.is_err() {
                return;
            }
            if let Some(table) = weak.upgrade() {
                table.resolve(
                    id,
                    Err(RequestError::Service(ServiceError {
                        kind: ReplyErrorKind::Network,
                        // Swift's `Int(timeout)` truncates.
                        message: format!("no answer within {} seconds", seconds as i64),
                    })),
                );
            }
        })
    }

    /// The caller-drop handler's work: forgets request `id` if it is still waiting. One already answered needs nothing.
    pub fn cancel(&self, id: u64) {
        let found = self.lock().pending.remove(&id);
        if let Some(entry) = found {
            if let Some(timer) = entry.timer {
                timer.abort();
            }
            // The receiver is gone or going; the answer is never read.
            let _ = entry.answer.send(Err(RequestError::Cancelled));
        }
    }

    fn resolve(&self, id: u64, result: Result<Value, RequestError>) {
        let Some(entry) = self.lock().pending.remove(&id) else {
            return;
        };
        if let Some(timer) = entry.timer {
            timer.abort();
        }
        let _ = entry.answer.send(result);
    }

    /// The host's answer to request `id`. Unknown and already resolved ids are ignored; a reply that is not a
    /// well-formed v1 reply fails the request with `failed`.
    pub fn fulfill(&self, id: u64, reply: &str) {
        match WireReply::parse(reply) {
            WireReply::Result(value) => self.resolve(id, Ok(value)),
            WireReply::Error { kind, message } => {
                self.resolve(id, Err(RequestError::from_kind(kind, &message)));
            }
        }
    }

    /// Fails every pending request with `kind` and every later one before it is sent.
    pub fn fail_all(&self, kind: ReplyErrorKind) {
        let entries: Vec<Pending> = {
            let mut state = self.lock();
            state.closed_kind = Some(kind);
            std::mem::take(&mut state.pending).into_values().collect()
        };
        for entry in entries {
            if let Some(timer) = entry.timer {
                timer.abort();
            }
            let _ = entry.answer.send(Err(RequestError::from_kind(
                kind,
                "the engine is shutting down",
            )));
        }
    }

    /// Sends `payload` (none when `None`) and decodes the result as `T`. A result that does not decode is a `failed`
    /// error.
    pub async fn call<T: DeserializeOwned>(
        &self,
        message_type: &str,
        payload: Option<Value>,
        timeout: Option<f64>,
    ) -> Result<T, RequestError> {
        let value = self.request(message_type, payload, timeout).await?;
        serde_json::from_value(value).map_err(|_| {
            RequestError::Service(ServiceError {
                kind: ReplyErrorKind::Failed,
                message: format!("the {message_type} result has an unexpected shape"),
            })
        })
    }
}
