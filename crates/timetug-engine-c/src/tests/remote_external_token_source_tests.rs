//! Port of `RemoteExternalTokenSourceTests.swift`: the host's account service (Windows' Web Account Manager) as the
//! library's `ExternalTokenSource`, over `service.token.*`. The mapping is the spec's table.
//!
//! A cancelled Swift task is an aborted Rust task here (`JoinHandle::abort`): its future is dropped, and the drop guard
//! of the sign-in sends `service.token.cancelSignIn`. The aborted task's own result is a `JoinError`, where Swift's
//! was `CancellationError`.

use std::sync::Arc;

use calendar_core::external_tokens::{
    ExternalAccessToken, ExternalAccount, ExternalSignInRequest, ExternalSignInResult,
    ExternalTokenSource,
};
use calendar_core::failure::{Failure, Outcome};
use calendar_core::source_types::SourceError;
use jiff::Timestamp;
use serde_json::json;
use timetug_engine::clock::EngineClock;
use timetug_engine::diagnostic::EngineDiagnostic;
use timetug_engine_protocol::envelope::{Envelope, MessageId};
use timetug_engine_protocol::wire_types::WireDates;

use super::support::{LockedBox, poll, spawn};
use crate::manual_engine_clock::ManualEngineClock;
use crate::remote_host_services::RemoteExternalTokenSource;
use crate::request_table::RequestTable;

const OK_EXPIRES: &str = "2026-10-09T09:53:20.000Z";

struct Fixture {
    source: Arc<RemoteExternalTokenSource>,
    table: Arc<RequestTable>,
    sent: Arc<LockedBox<Vec<Envelope>>>,
    clock: Arc<ManualEngineClock>,
    diagnostics: Arc<LockedBox<Vec<EngineDiagnostic>>>,
}

fn make() -> Fixture {
    let sent = LockedBox::new(Vec::<Envelope>::new());
    let diagnostics = LockedBox::new(Vec::<EngineDiagnostic>::new());
    let clock = Arc::new(ManualEngineClock::new(
        Timestamp::from_second(1_760_000_000).unwrap(),
    ));
    let recorder = sent.clone();
    let table = RequestTable::new(
        Arc::new(move |envelope| recorder.mutate(|all| all.push(envelope))),
        clock.clone() as Arc<dyn EngineClock>,
    );
    let sink = diagnostics.clone();
    let source = Arc::new(RemoteExternalTokenSource::new(
        table.clone(),
        Arc::new(move |diagnostic| sink.mutate(|all| all.push(diagnostic))),
    ));
    Fixture {
        source,
        table,
        sent,
        clock,
        diagnostics,
    }
}

fn account() -> ExternalAccount {
    ExternalAccount {
        id: "acct-1".to_string(),
        email: Some("me@example.com".to_string()),
        connection_id: Some("conn-1".to_string()),
    }
}

fn scopes() -> Vec<String> {
    vec!["User.Read".to_string(), "Calendars.Read".to_string()]
}

fn request_id(envelope: &Envelope) -> u64 {
    match &envelope.id {
        Some(MessageId::Number(id)) => *id,
        other => panic!("no numeric request id: {other:?}"),
    }
}

/// Waits for request number `index`, answers it with `reply`, and returns the request.
async fn answer_at(fixture: &Fixture, reply: &str, index: usize) -> Envelope {
    poll(&format!("request {index} is sent"), || {
        fixture.sent.value().len() > index
    })
    .await;
    let envelope = fixture.sent.value()[index].clone();
    fixture.table.fulfill(request_id(&envelope), reply);
    envelope
}

async fn answer(fixture: &Fixture, reply: &str) -> Envelope {
    answer_at(fixture, reply, 0).await
}

fn get(fixture: &Fixture, force: bool) -> tokio::task::JoinHandle<Outcome<ExternalAccessToken>> {
    let source = fixture.source.clone();
    spawn(async move { source.access_token(&account(), &scopes(), force).await })
}

fn sign_in(
    fixture: &Fixture,
    account: Option<ExternalAccount>,
) -> tokio::task::JoinHandle<Outcome<ExternalSignInResult>> {
    let source = fixture.source.clone();
    spawn(async move {
        let request = ExternalSignInRequest {
            email: account.as_ref().and_then(|a| a.email.clone()),
            account,
            scopes: scopes(),
        };
        source.sign_in(&request).await
    })
}

fn invalid(text: &str) -> Failure {
    Failure::Source(SourceError::InvalidResponse(text.to_string()))
}

fn incomplete() -> Failure {
    invalid("the host's token answer was incomplete")
}

// MARK: - service.token.get

#[tokio::test]
async fn get_ok_returns_the_token_and_expiry() {
    let fixture = make();
    let result = get(&fixture, true);
    let request = answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    let token = result.await.unwrap().unwrap();
    assert_eq!(token.access_token, "tok");
    assert_eq!(Some(token.expires_at), WireDates::date(OK_EXPIRES));
    assert_eq!(request.r#type, "service.token.get");
    let payload = request.payload.unwrap();
    assert_eq!(payload["connectionID"], json!("conn-1"));
    assert_eq!(payload["accountID"], json!("acct-1"));
    assert_eq!(payload["email"], json!("me@example.com"));
    assert_eq!(payload["forceRefresh"], json!(true));
    assert_eq!(payload["scopes"], json!(["User.Read", "Calendars.Read"]));
}

#[tokio::test]
async fn get_interaction_required_is_auth_expired() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"interactionRequired","reason":"signedOut"}}"#,
    )
    .await;
    assert_eq!(
        result.await.unwrap().unwrap_err(),
        Failure::Source(SourceError::AuthExpired)
    );
}

#[tokio::test]
async fn get_unavailable_is_invalid_response() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"unavailable","reason":"brokerUnavailable"}}"#,
    )
    .await;
    assert_eq!(
        result.await.unwrap().unwrap_err(),
        invalid("Windows sign-in is not available")
    );
}

#[tokio::test]
async fn get_ok_without_access_token_is_invalid_response() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    assert_eq!(result.await.unwrap().unwrap_err(), incomplete());
}

#[tokio::test]
async fn get_ok_with_an_empty_access_token_is_invalid_response() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accessToken":"","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    assert_eq!(result.await.unwrap().unwrap_err(), incomplete());
}

#[tokio::test]
async fn get_ok_with_garbage_expires_at_is_invalid_response() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accessToken":"tok","expiresAt":"tomorrow"}}"#,
    )
    .await;
    assert_eq!(result.await.unwrap().unwrap_err(), incomplete());
}

#[tokio::test]
async fn get_unknown_status_is_invalid_response() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"somethingNew","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    assert_eq!(result.await.unwrap().unwrap_err(), incomplete());
}

#[tokio::test]
async fn get_result_of_the_wrong_shape_is_invalid_response() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(&fixture, r#"{"v":1,"result":"nope"}"#).await;
    match result.await.unwrap() {
        Err(Failure::Source(SourceError::InvalidResponse(_))) => {}
        other => panic!("a result of the wrong shape was accepted: {other:?}"),
    }
}

#[tokio::test]
async fn get_network_error_is_source_network() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"error":{"kind":"network","message":"offline"}}"#,
    )
    .await;
    match result.await.unwrap() {
        Err(Failure::Source(SourceError::Network(_))) => {}
        other => panic!("expected network, got {other:?}"),
    }
}

#[tokio::test]
async fn get_cancelled_error_is_cancellation() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(&fixture, r#"{"v":1,"error":{"kind":"cancelled"}}"#).await;
    assert_eq!(result.await.unwrap().unwrap_err(), Failure::Cancelled);
}

#[tokio::test]
async fn get_other_error_kinds_are_invalid_response_without_the_hosts_text() {
    for kind in ["failed", "denied", "notFound"] {
        let fixture = make();
        let result = get(&fixture, false);
        answer(
            &fixture,
            &format!(r#"{{"v":1,"error":{{"kind":"{kind}","message":"secret-token-123"}}}}"#),
        )
        .await;
        match result.await.unwrap() {
            Err(Failure::Source(SourceError::InvalidResponse(text))) => {
                assert!(!text.contains("secret-token-123"), "{kind}: {text}");
            }
            other => panic!("expected invalidResponse for {kind}, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn get_times_out_after_sixty_seconds_on_the_engine_clock() {
    let fixture = make();
    let result = get(&fixture, false);
    poll("the request is sent", || !fixture.sent.value().is_empty()).await;
    poll("the timer sleeps", || fixture.clock.pending_sleeps() == 1).await;
    fixture.clock.advance(59.0);
    assert_eq!(fixture.clock.pending_sleeps(), 1);
    fixture.clock.advance(2.0);
    match result.await.unwrap() {
        Err(Failure::Source(SourceError::Network(_))) => {}
        other => panic!("expected network, got {other:?}"),
    }
}

// MARK: - service.token.signIn

const SIGN_IN_OK: &str = r#"{"v":1,"result":{"status":"ok","accountID":"acct-1","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#;

#[tokio::test]
async fn sign_in_has_no_timeout() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    poll("the request is sent", || !fixture.sent.value().is_empty()).await;
    fixture.clock.advance(600.0);
    assert_eq!(fixture.clock.pending_sleeps(), 0);
    assert_eq!(
        fixture.table.pending_count(),
        1,
        "still waiting for the person"
    );
    fixture
        .table
        .fulfill(request_id(&fixture.sent.value()[0]), SIGN_IN_OK);
    result.await.unwrap().unwrap();
}

#[tokio::test]
async fn sign_in_ok_returns_account_and_token() {
    let fixture = make();
    let result = sign_in(&fixture, Some(account()));
    let request = answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accountID":"acct-2","accessToken":"tok","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    let signed_in = result.await.unwrap().unwrap();
    assert_eq!(signed_in.account.id, "acct-2");
    assert_eq!(signed_in.token.access_token, "tok");
    assert_eq!(
        Some(signed_in.token.expires_at),
        WireDates::date(OK_EXPIRES)
    );
    assert_eq!(request.r#type, "service.token.signIn");
    let payload = request.payload.unwrap();
    assert_eq!(payload["accountID"], json!("acct-1"));
    assert_eq!(payload["email"], json!("me@example.com"));
    assert_eq!(payload["scopes"], json!(["User.Read", "Calendars.Read"]));
    let sign_in_id = payload["signInID"].as_str().expect("no signInID");
    assert!(!sign_in_id.is_empty());
}

#[tokio::test]
async fn first_sign_in_sends_no_account() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    let request = answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    result.await.unwrap().unwrap();
    let payload = request.payload.unwrap();
    assert!(payload.get("accountID").is_none());
    assert!(payload.get("email").is_none());
}

#[tokio::test]
async fn sign_in_ids_are_fresh_per_call() {
    let fixture = make();
    let reply = r#"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#;
    let first = sign_in(&fixture, None);
    answer_at(&fixture, reply, 0).await;
    first.await.unwrap().unwrap();
    let second = sign_in(&fixture, None);
    answer_at(&fixture, reply, 1).await;
    second.await.unwrap().unwrap();
    let sent = fixture.sent.value();
    assert_ne!(
        sent[0].payload.as_ref().unwrap()["signInID"],
        sent[1].payload.as_ref().unwrap()["signInID"]
    );
}

#[tokio::test]
async fn sign_in_blocked_is_admin_approval_required() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"blocked","reason":"adminConsentRequired"}}"#,
    )
    .await;
    assert_eq!(
        result.await.unwrap().unwrap_err(),
        invalid("admin approval is required")
    );
}

#[tokio::test]
async fn sign_in_unavailable_is_invalid_response() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"unavailable","reason":"brokerUnavailable"}}"#,
    )
    .await;
    assert_eq!(
        result.await.unwrap().unwrap_err(),
        invalid("Windows sign-in is not available")
    );
}

#[tokio::test]
async fn sign_in_ok_without_an_account_id_is_invalid_response() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    assert_eq!(result.await.unwrap().unwrap_err(), incomplete());
}

#[tokio::test]
async fn sign_in_network_error_is_source_network_and_cancelled_is_cancellation() {
    let fixture = make();
    let network = sign_in(&fixture, None);
    answer_at(&fixture, r#"{"v":1,"error":{"kind":"network"}}"#, 0).await;
    match network.await.unwrap() {
        Err(Failure::Source(SourceError::Network(_))) => {}
        other => panic!("expected network, got {other:?}"),
    }
    let cancelled = sign_in(&fixture, None);
    answer_at(&fixture, r#"{"v":1,"error":{"kind":"cancelled"}}"#, 1).await;
    assert_eq!(cancelled.await.unwrap().unwrap_err(), Failure::Cancelled);
}

#[tokio::test]
async fn cancelling_the_signing_in_task_sends_cancel_sign_in_with_the_same_sign_in_id() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    poll("the sign-in is sent", || !fixture.sent.value().is_empty()).await;
    let sign_in_id = fixture.sent.value()[0].payload.as_ref().unwrap()["signInID"].clone();
    result.abort();
    assert!(result.await.unwrap_err().is_cancelled());
    poll("the cancel is sent", || fixture.sent.value().len() == 2).await;
    let sent = fixture.sent.value();
    assert_eq!(sent[1].r#type, "service.token.cancelSignIn");
    assert_eq!(sent[1].payload.as_ref().unwrap()["signInID"], sign_in_id);
    // The host answers the cancel; nothing is left pending.
    fixture
        .table
        .fulfill(request_id(&sent[1]), r#"{"v":1,"result":{}}"#);
    poll("nothing pending", || fixture.table.pending_count() == 0).await;
}

#[tokio::test]
async fn a_failing_cancel_sign_in_is_silent() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    poll("the sign-in is sent", || !fixture.sent.value().is_empty()).await;
    result.abort();
    let _ = result.await;
    poll("the cancel is sent", || fixture.sent.value().len() == 2).await;
    fixture.table.fulfill(
        request_id(&fixture.sent.value()[1]),
        r#"{"v":1,"error":{"kind":"failed","message":"x"}}"#,
    );
    poll("nothing pending", || fixture.table.pending_count() == 0).await;
}

#[tokio::test]
async fn late_sign_in_answer_after_cancel_is_dropped() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    poll("the sign-in is sent", || !fixture.sent.value().is_empty()).await;
    let id = request_id(&fixture.sent.value()[0]);
    result.abort();
    assert!(result.await.unwrap_err().is_cancelled());
    // The dialog was answered after all: the engine ignores it, so no account results.
    fixture.table.fulfill(id, SIGN_IN_OK);
    poll("the cancelled request is forgotten", || {
        fixture.table.pending_count() <= 1
    })
    .await;
    // Only the cancelSignIn request (sent by the guard) may still be waiting for its answer.
    assert!(fixture.table.pending_count() <= 1);
}

#[tokio::test]
async fn no_cancel_is_sent_for_a_sign_in_that_already_answered() {
    let fixture = make();
    let result = sign_in(&fixture, None);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"ok","accountID":"a","accessToken":"t","expiresAt":"2026-10-09T09:53:20.000Z"}}"#,
    )
    .await;
    result.await.unwrap().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(fixture.sent.value().len(), 1);
}

// MARK: - Diagnostics

#[tokio::test]
async fn a_documented_reason_is_logged_as_a_code_only() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"interactionRequired","reason":"mfa"}}"#,
    )
    .await;
    let _ = result.await.unwrap();
    let messages: Vec<String> = fixture
        .diagnostics
        .value()
        .into_iter()
        .map(|d| d.message)
        .collect();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("mfa"));
}

#[tokio::test]
async fn an_undocumented_reason_is_dropped() {
    let fixture = make();
    let result = get(&fixture, false);
    answer(
        &fixture,
        r#"{"v":1,"result":{"status":"interactionRequired","reason":"me@example.com is locked out"}}"#,
    )
    .await;
    let _ = result.await.unwrap();
    assert!(fixture.diagnostics.value().is_empty());
}

#[tokio::test]
async fn diagnostics_never_contain_token_or_account_id_or_email() {
    let sentinels = ["SENTINEL-TOKEN-9", "acct-1", "me@example.com", "conn-1"];
    let replies = [
        r#"{"v":1,"result":{"status":"ok","accessToken":"SENTINEL-TOKEN-9","expiresAt":"2026-10-09T09:53:20.000Z","reason":"consent"}}"#,
        r#"{"v":1,"result":{"status":"interactionRequired","reason":"SENTINEL-TOKEN-9 acct-1 me@example.com"}}"#,
        r#"{"v":1,"error":{"kind":"failed","message":"boom SENTINEL-TOKEN-9 acct-1 me@example.com conn-1"}}"#,
        r#"{"v":1,"error":{"kind":"network","message":"SENTINEL-TOKEN-9"}}"#,
    ];
    for reply in replies {
        let fixture = make();
        let result = get(&fixture, false);
        answer(&fixture, reply).await;
        let _ = result.await.unwrap();
        let text: String = fixture
            .diagnostics
            .value()
            .iter()
            .map(|d| format!("{} {}", d.category, d.message))
            .collect::<Vec<_>>()
            .join("\n");
        for sentinel in sentinels {
            assert!(
                !text.contains(sentinel),
                "{sentinel} leaked into a diagnostic"
            );
        }
    }
}
