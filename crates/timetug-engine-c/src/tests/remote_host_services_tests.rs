//! Port of `RemoteHostServicesTests.swift`.
//!
//! Swift's cancellation bookkeeping (`trackedIDCount`, `cancel(id:)` run late) has no Rust counterpart: a request is
//! registered before its first suspension point, so there is no window in which it is issued but not registered, and a
//! cancelled caller is a dropped future whose guard removes the entry. The tests that pinned the bookkeeping pin the
//! same outcome instead: after the answer and a late `cancel`, nothing is left pending and no sleeper is left.

use std::collections::BTreeMap;
use std::sync::Arc;

use calendar_core::connection::CredentialStore;
use calendar_core::failure::Failure;
use calendar_core::http::{HttpRequest, HttpTransport};
use calendar_core::source_types::SourceError;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use serde_json::{Value, json};
use timetug_engine::clock::EngineClock;
use timetug_engine_protocol::envelope::{Envelope, MessageId};
use timetug_engine_protocol::service_messages::ReplyErrorKind;
use uuid::Uuid;

use super::support::{LockedBox, poll, spawn};
use crate::manual_engine_clock::ManualEngineClock;
use crate::remote_host_services::{RemoteCredentialStore, RemoteHttpTransport};
use crate::request_table::{RequestError, RequestTable, ServiceError};

fn start() -> Timestamp {
    Timestamp::from_second(1_760_000_000).unwrap()
}

/// A table whose sent envelopes are recorded, on a manual clock.
fn make_table() -> (
    Arc<RequestTable>,
    Arc<LockedBox<Vec<Envelope>>>,
    Arc<ManualEngineClock>,
) {
    let sent = LockedBox::new(Vec::<Envelope>::new());
    let clock = Arc::new(ManualEngineClock::new(start()));
    let recorder = sent.clone();
    let table = RequestTable::new(
        Arc::new(move |envelope| recorder.mutate(|all| all.push(envelope))),
        clock.clone() as Arc<dyn EngineClock>,
    );
    (table, sent, clock)
}

fn request_id(envelope: &Envelope) -> Option<u64> {
    match &envelope.id {
        Some(MessageId::Number(id)) => Some(*id),
        _ => None,
    }
}

fn plain_request(
    table: &Arc<RequestTable>,
    message_type: &'static str,
    timeout: Option<f64>,
) -> tokio::task::JoinHandle<Result<Value, RequestError>> {
    let table = table.clone();
    spawn(async move { table.request(message_type, None, timeout).await })
}

#[tokio::test]
async fn reply_resolves_request() {
    let (table, sent, _) = make_table();
    let result = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    let first = sent.value()[0].clone();
    let id = request_id(&first).unwrap();
    assert_eq!(first.r#type, "service.x");
    table.fulfill(id, r#"{"v":1,"result":{"a":1}}"#);
    assert_eq!(result.await.unwrap().unwrap(), json!({"a": 1}));
}

#[tokio::test]
async fn request_ids_are_distinct_and_non_zero() {
    let (table, sent, _) = make_table();
    let a = plain_request(&table, "service.a", None);
    let b = plain_request(&table, "service.b", None);
    poll("both requests are sent", || sent.value().len() == 2).await;
    let envelopes = sent.value();
    let ids: Vec<u64> = envelopes.iter().filter_map(request_id).collect();
    let distinct: std::collections::BTreeSet<u64> = ids.iter().copied().collect();
    assert_eq!(distinct.len(), 2);
    assert!(!ids.contains(&0));
    for envelope in &envelopes {
        table.fulfill(
            request_id(envelope).unwrap(),
            &format!(r#"{{"v":1,"result":"{}"}}"#, envelope.r#type),
        );
    }
    assert_eq!(a.await.unwrap().unwrap(), json!("service.a"));
    assert_eq!(b.await.unwrap().unwrap(), json!("service.b"));
}

#[tokio::test]
async fn error_reply_throws_the_kind() {
    let (table, sent, _) = make_table();
    let result = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    table.fulfill(
        request_id(&sent.value()[0]).unwrap(),
        r#"{"v":1,"error":{"kind":"denied","message":"no"}}"#,
    );
    assert_eq!(
        result.await.unwrap(),
        Err(RequestError::Service(ServiceError {
            kind: ReplyErrorKind::Denied,
            message: "no".to_string()
        }))
    );
}

#[tokio::test]
async fn unsupported_reply_version_fails_the_request() {
    let (table, sent, _) = make_table();
    let result = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    table.fulfill(
        request_id(&sent.value()[0]).unwrap(),
        r#"{"v":2,"result":{}}"#,
    );
    match result.await.unwrap() {
        Err(RequestError::Service(error)) => assert_eq!(error.kind, ReplyErrorKind::Failed),
        other => panic!("a v2 reply did not fail the request with `failed`: {other:?}"),
    }
}

#[tokio::test]
async fn unknown_and_repeated_replies_are_ignored() {
    let (table, sent, _) = make_table();
    table.fulfill(999, r#"{"v":1,"result":1}"#);
    let result = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    let id = request_id(&sent.value()[0]).unwrap();
    table.fulfill(id, r#"{"v":1,"result":1}"#);
    table.fulfill(id, r#"{"v":1,"result":2}"#);
    table.fulfill(id, "not json");
    assert_eq!(result.await.unwrap().unwrap(), json!(1));
}

#[tokio::test]
async fn fail_all_cancels_pending_and_later_requests() {
    let (table, sent, _) = make_table();
    let result = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    table.fail_all(ReplyErrorKind::Cancelled);
    assert_eq!(result.await.unwrap(), Err(RequestError::Cancelled));
    assert_eq!(
        table.request("service.y", None, None).await,
        Err(RequestError::Cancelled)
    );
    assert_eq!(
        sent.value().len(),
        1,
        "a request after failAll is never sent"
    );
}

#[tokio::test]
async fn cancelling_the_caller_cancels_the_request() {
    let (table, sent, _) = make_table();
    let task = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    poll("the request is forgotten", || table.pending_count() == 0).await;
    // The host's late answer is ignored.
    table.fulfill(
        request_id(&sent.value()[0]).unwrap(),
        r#"{"v":1,"result":1}"#,
    );
    assert_eq!(table.pending_count(), 0);
}

/// A cancellation that runs after the answer (a reply or failAll racing the cancel) keeps nothing.
#[tokio::test]
async fn late_cancellation_keeps_no_bookkeeping() {
    let (table, sent, clock) = make_table();
    let result = plain_request(&table, "service.x", None);
    poll("the request is sent", || !sent.value().is_empty()).await;
    let id = request_id(&sent.value()[0]).unwrap();
    table.fulfill(id, r#"{"v":1,"result":1}"#);
    assert_eq!(result.await.unwrap().unwrap(), json!(1));
    table.cancel(id);
    assert_eq!(table.pending_count(), 0);

    let sleeper_clock = clock.clone();
    let sleeper = spawn(async move {
        sleeper_clock
            .sleep_until(Timestamp::from_second(1_760_000_010).unwrap())
            .await
    });
    poll("the sleep is registered", || clock.pending_sleeps() == 1).await;
    clock.advance(10.0);
    sleeper.await.unwrap().unwrap();
    clock.cancel(1);
    assert_eq!(clock.pending_sleeps(), 0);
}

#[tokio::test]
async fn timeout_follows_the_engine_clock() {
    let (table, sent, clock) = make_table();
    let result = plain_request(&table, "service.x", Some(60.0));
    poll("the request is sent", || !sent.value().is_empty()).await;
    poll("the timer sleeps", || clock.pending_sleeps() == 1).await;
    clock.advance(59.0);
    assert_eq!(clock.pending_sleeps(), 1);
    clock.advance(2.0);
    match result.await.unwrap() {
        Err(RequestError::Service(error)) => assert_eq!(error.kind, ReplyErrorKind::Network),
        other => panic!("the request did not time out: {other:?}"),
    }
}

#[tokio::test]
async fn answered_request_stops_its_timer() {
    let (table, sent, clock) = make_table();
    let result = plain_request(&table, "service.x", Some(60.0));
    poll("the request is sent", || !sent.value().is_empty()).await;
    table.fulfill(
        request_id(&sent.value()[0]).unwrap(),
        r#"{"v":1,"result":true}"#,
    );
    assert_eq!(result.await.unwrap().unwrap(), json!(true));
    poll("the timer is cancelled", || clock.pending_sleeps() == 0).await;
}

fn get(url: &str) -> HttpRequest {
    HttpRequest::new(UrlString::parse(url).unwrap())
}

#[tokio::test]
async fn http_timeout_maps_to_network_error() {
    let (table, sent, clock) = make_table();
    let transport = RemoteHttpTransport::new(table);
    let response = spawn(async move { transport.send(get("https://example.com/a")).await });
    poll("the request is sent", || !sent.value().is_empty()).await;
    poll("the timer sleeps", || clock.pending_sleeps() == 1).await;
    clock.advance(61.0);
    match response.await.unwrap() {
        Err(Failure::Source(SourceError::Network(_))) => {}
        other => panic!("expected a network error, got {other:?}"),
    }
}

#[tokio::test]
async fn http_body_is_base64_on_the_wire() {
    let (table, sent, _) = make_table();
    let transport = RemoteHttpTransport::new(table.clone());
    let response = spawn(async move {
        let mut request = get("https://example.com/token");
        request.method = "POST".to_string();
        request.headers = BTreeMap::from([("Content-Type".to_string(), "text/plain".to_string())]);
        request.body = Some(b"hello".to_vec());
        transport.send(request).await
    });
    poll("the request is sent", || !sent.value().is_empty()).await;
    let envelope = sent.value()[0].clone();
    assert_eq!(envelope.r#type, "service.http.send");
    let payload = envelope.payload.clone().unwrap();
    assert_eq!(payload["body"], json!("aGVsbG8="));
    assert_eq!(payload["method"], json!("POST"));
    assert_eq!(payload["url"], json!("https://example.com/token"));
    assert_eq!(payload["headers"]["Content-Type"], json!("text/plain"));
    table.fulfill(
        request_id(&envelope).unwrap(),
        r#"{"v":1,"result":{"status":201,"headers":{"X-A":"b"},"body":"eyJvayI6dHJ1ZX0="}}"#,
    );
    let value = response.await.unwrap().unwrap();
    assert_eq!(value.status, 201);
    assert_eq!(value.header("x-a"), Some("b"));
    assert_eq!(String::from_utf8(value.body).unwrap(), r#"{"ok":true}"#);
}

#[tokio::test]
async fn http_transport_errors_are_network_errors() {
    let (table, sent, _) = make_table();
    let transport = RemoteHttpTransport::new(table.clone());
    let response = spawn(async move { transport.send(get("https://example.com/a")).await });
    poll("the request is sent", || !sent.value().is_empty()).await;
    table.fulfill(
        request_id(&sent.value()[0]).unwrap(),
        r#"{"v":1,"error":{"kind":"network","message":"offline"}}"#,
    );
    assert_eq!(
        response.await.unwrap().unwrap_err(),
        Failure::Source(SourceError::Network("network: offline".to_string()))
    );
}

/// Upper-case, as Swift's `UUID.uuidString`.
fn uuid_string(uuid: Uuid) -> String {
    uuid.hyphenated().to_string().to_uppercase()
}

#[tokio::test]
async fn credential_store_round_trip() {
    let (table, sent, _) = make_table();
    let store = Arc::new(RemoteCredentialStore::new(table.clone()));
    let revision = Uuid::new_v4();
    // A host that keeps one secret set.
    let answering_table = table.clone();
    let answering_sent = sent.clone();
    let answering = spawn(async move {
        let mut answered = 0;
        while answered < 3 {
            let pending: Vec<Envelope> =
                answering_sent.value().into_iter().skip(answered).collect();
            for envelope in pending {
                let Some(id) = request_id(&envelope) else {
                    continue;
                };
                match envelope.r#type.as_str() {
                    "service.credentials.get" => answering_table.fulfill(
                        id,
                        r#"{"v":1,"result":{"secrets":{"refresh_token":"r"}}}"#,
                    ),
                    "service.credentials.snapshot" => answering_table.fulfill(
                        id,
                        &format!(
                            r#"{{"v":1,"result":{{"snapshot":{{"secrets":{{"refresh_token":"r"}},"revision":"{}"}}}}}}"#,
                            uuid_string(revision)
                        ),
                    ),
                    "service.credentials.updateRefreshToken" => {
                        answering_table.fulfill(id, r#"{"v":1,"result":{"updated":true}}"#);
                    }
                    _ => {}
                }
                answered += 1;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });
    assert_eq!(
        store.secrets("c1").await.unwrap(),
        Some(BTreeMap::from([(
            "refresh_token".to_string(),
            "r".to_string()
        )]))
    );
    let snapshot = store.credential_snapshot("c1").await.unwrap().unwrap();
    assert_eq!(snapshot.revision, revision);
    assert!(
        store
            .update_refresh_token("n", "c1", revision)
            .await
            .unwrap()
    );
    answering.await.unwrap();
    let all = sent.value();
    assert_eq!(
        all[2].payload.as_ref().unwrap()["expectedRevision"],
        json!(uuid_string(revision))
    );
    assert_eq!(all[0].payload, Some(json!({"connectionID": "c1"})));
}

/// The revision is the host's string: a lower-case one (.NET's `Guid.ToString()`) must come back exactly as the host
/// sent it, not re-spelled as upper case, or the host's compare-and-set never matches.
#[tokio::test]
async fn credential_revision_comes_back_as_the_host_spelled_it() {
    let (table, sent, _) = make_table();
    let store = Arc::new(RemoteCredentialStore::new(table.clone()));
    let spelled = uuid_string(Uuid::new_v4()).to_lowercase();
    let answering_table = table.clone();
    let answering_sent = sent.clone();
    let answering_spelled = spelled.clone();
    let answering = spawn(async move {
        let mut answered = 0;
        while answered < 2 {
            let pending: Vec<Envelope> =
                answering_sent.value().into_iter().skip(answered).collect();
            for envelope in pending {
                let Some(id) = request_id(&envelope) else {
                    continue;
                };
                if envelope.r#type == "service.credentials.snapshot" {
                    answering_table.fulfill(
                        id,
                        &format!(
                            r#"{{"v":1,"result":{{"snapshot":{{"secrets":{{}},"revision":"{answering_spelled}"}}}}}}"#
                        ),
                    );
                } else {
                    answering_table.fulfill(id, r#"{"v":1,"result":{"updated":true}}"#);
                }
                answered += 1;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });
    let snapshot = store.credential_snapshot("c1").await.unwrap().unwrap();
    assert!(
        store
            .update_refresh_token("n", "c1", snapshot.revision)
            .await
            .unwrap()
    );
    answering.await.unwrap();
    assert_eq!(
        sent.value()[1].payload.as_ref().unwrap()["expectedRevision"],
        json!(spelled)
    );
}
