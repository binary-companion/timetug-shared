//! Port of `FixtureTests.swift`: the golden fixtures of `Conformance/Fixtures/golden` against the wire types, the
//! command decoder and the message catalog. `TT_BASELINE_FIXTURES=<dir>` switches to baseline mode exactly as the Swift
//! test does (the comparison becomes the superset relation of `AGENTS.md`).

use std::collections::BTreeSet;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use timetug_engine::settings::EngineSettings;
use timetug_engine::state::EngineState;

use super::support::conformance::{self, envelope, golden, golden_text, golden_value};
use super::support::golden_messages;
use crate::command_mapping::{CommandDecoder, CommandOutcome};
use crate::envelope::{Envelope, MessageId};
use crate::message_catalog::{MessageCatalog, WireReady, WireReadyCapabilities};
use crate::service_messages::*;
use crate::wire_types::*;

fn payload_of(name: &str) -> Value {
    envelope(name)
        .payload
        .unwrap_or_else(|| panic!("{name}: no payload"))
}

fn text(name: &str) -> String {
    golden_text(name)
}

/// Not in baseline mode: a type added since the baseline has no baseline fixture (additions are allowed).
#[test]
fn every_catalog_type_has_a_fixture() {
    if conformance::is_baseline() {
        return;
    }
    let names: BTreeSet<String> = conformance::golden_files().into_iter().collect();
    for kind in MessageCatalog::all() {
        assert!(
            names.contains(&format!("{kind}.json")),
            "missing fixture for {kind}"
        );
    }
}

#[test]
fn no_orphan_fixtures() {
    let all = MessageCatalog::all();
    let types: BTreeSet<&str> = all.iter().copied().collect();
    for name in conformance::golden_files() {
        if name.starts_with("edge-") {
            continue;
        }
        let stem = &name[..name.len() - 5];
        assert!(types.contains(stem), "{name} is not a catalog type");
    }
    assert_eq!(types.len(), all.len(), "duplicate type in the catalog");
}

#[test]
fn fixture_type_matches_its_file_name() {
    for kind in MessageCatalog::all() {
        assert_eq!(envelope(&format!("{kind}.json")).r#type, kind);
    }
}

#[test]
fn every_fixture_round_trips_as_parsed_json() {
    let mut checked = 0;
    for name in conformance::golden_files() {
        if name.starts_with("edge-") {
            continue;
        }
        let data = golden(&name);
        let env = Envelope::from_slice(&data).unwrap();
        let again = env.to_value();
        let fixture: Value = serde_json::from_slice(&data).unwrap();
        assert!(conformance::matches(&again, &fixture), "{name}");
        checked += 1;
    }
    assert!(checked > 0);
}

fn round_trip<T: Serialize + DeserializeOwned>(payload: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(payload.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&typed).map_err(|e| e.to_string())
}

type RoundTrip = fn(&Value) -> Result<Value, String>;

/// The payload types of the contract: every fixture of a listed type decodes into its wire type and re-encodes to the
/// same JSON (in baseline mode: to a superset of the baseline's, so an added field passes and a removed, renamed or
/// retyped one fails).
fn payload_types() -> Vec<(&'static str, RoundTrip)> {
    vec![
        ("ready", round_trip::<WireReady>),
        ("state", round_trip::<WireState>),
        ("edge-state-", round_trip::<WireState>),
        ("event.takeoverDue", round_trip::<WireTakeoverRequest>),
        ("event.widgetSnapshot", round_trip::<WireWidgetSnapshot>),
        ("event.diagnostic", round_trip::<WireDiagnostic>),
        ("edge-event-diagnostic-", round_trip::<WireDiagnostic>),
        ("edge-settings-patch-", round_trip::<WireSettingsPatch>),
        (
            "service.credentials.get",
            round_trip::<CredentialsKeyRequest>,
        ),
        (
            "service.credentials.remove",
            round_trip::<CredentialsKeyRequest>,
        ),
        (
            "service.credentials.set",
            round_trip::<CredentialsSetRequest>,
        ),
        (
            "service.credentials.updateRefreshToken",
            round_trip::<CredentialsUpdateRefreshTokenRequest>,
        ),
        ("service.http.send", round_trip::<HttpSendRequest>),
        ("service.auth.authorize", round_trip::<AuthAuthorizeRequest>),
        (
            "service.auth.closeRedirect",
            round_trip::<AuthCloseRedirectRequest>,
        ),
        (
            "service.auth.promptCredentials",
            round_trip::<AuthPromptCredentialsRequest>,
        ),
        ("service.signIn.prepare", round_trip::<SignInPrepareRequest>),
        ("service.text.generate", round_trip::<TextGenerateRequest>),
        ("service.token.get", round_trip::<TokenGetRequest>),
        ("service.token.signIn", round_trip::<TokenSignInRequest>),
        (
            "service.token.cancelSignIn",
            round_trip::<TokenCancelSignInRequest>,
        ),
        ("edge-ready-", round_trip::<WireReady>),
    ]
}

#[test]
fn typed_payloads_round_trip() {
    let table = payload_types();
    let mut checked = 0;
    for name in conformance::golden_files() {
        let stem = &name[..name.len() - 5];
        let Some((_, entry)) = table.iter().find(|(prefix, _)| {
            if prefix.ends_with('-') {
                stem.starts_with(prefix)
            } else {
                stem == *prefix
            }
        }) else {
            continue;
        };
        let payload = payload_of(&name);
        let again = entry(&payload).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            conformance::matches(&again, &payload),
            "{name} does not round-trip through its wire type"
        );
        checked += 1;
    }
    assert!(checked >= 15, "only {checked} fixtures had a wire type");
}

#[test]
fn covers_allows_added_fields_only() {
    // `JSONValue.covers` of the Swift test support is `compat::is_superset`; the full set of cases is in
    // `compat_tests`, these are the six of the Swift test.
    use crate::compat::is_superset;
    let base = json!({"a": 1, "list": [{"b": "x"}]});
    assert!(is_superset(
        &json!({"a": 1, "new": true, "list": [{"b": "x", "c": null}]}),
        &base
    ));
    // removed
    assert!(!is_superset(&json!({"list": [{"b": "x"}]}), &base));
    // retyped
    assert!(!is_superset(
        &json!({"a": "1", "list": [{"b": "x"}]}),
        &base
    ));
    // changed
    assert!(!is_superset(&json!({"a": 2, "list": [{"b": "x"}]}), &base));
    // shortened
    assert!(!is_superset(&json!({"a": 1, "list": []}), &base));
    assert!(!is_superset(
        &json!({"a": 1, "list": [{"b": "x"}, null]}),
        &base
    ));
}

#[test]
fn engine_messages_match_their_golden_fixtures() {
    let all = golden_messages::all();
    assert_eq!(all.len(), 11);
    for (name, message) in all {
        let from_engine = message.to_value();
        let from_file = golden_value(&format!("{name}.json"));
        assert!(
            conformance::matches(&from_engine, &from_file),
            "{name} drifted from its fixture"
        );
    }
}

#[test]
fn state_fixture_decodes_into_the_wire_type() {
    let state: WireState = serde_json::from_value(payload_of("state.json")).unwrap();
    assert_eq!(state.revision, 12);
    assert_eq!(state.events.len(), 3);
    assert!(state.events.iter().any(|e| e.is_all_day
        && e.start_date.as_deref() == Some("2026-10-16")
        && e.start.is_none()));
    assert!(state.events.iter().any(|e| !e.members.is_empty()));
    assert_eq!(state.candidates.len(), 1);
}

#[test]
fn ready_fixture_decodes() {
    let ready: WireReady = serde_json::from_value(payload_of("ready.json")).unwrap();
    assert_eq!(ready.protocol_versions, [1]);
    assert!(!ready.capabilities.system_calendar);
}

#[test]
fn every_command_fixture_is_accepted() {
    let state = golden_messages::state();
    // The fixtures name event ids from the golden state.
    let decoder = CommandDecoder::new(true);
    for kind in MessageCatalog::HOST_TO_ENGINE {
        let outcome = decoder.decode(&envelope(&format!("{kind}.json")), &state);
        if let CommandOutcome::Rejected(d) = outcome {
            panic!("{kind} rejected: {}", d.message);
        }
    }
}

#[test]
fn unknown_type_fixture_is_rejected_not_thrown() {
    let outcome = CommandDecoder::new(false)
        .decode(&envelope("edge-unknown-type.json"), &EngineState::empty());
    let CommandOutcome::Rejected(d) = outcome else {
        panic!("not rejected");
    };
    assert_eq!(d.message, "unknown command type");
}

#[test]
fn version_two_fixture_is_rejected() {
    let outcome =
        CommandDecoder::new(false).decode(&envelope("edge-version-2.json"), &EngineState::empty());
    let CommandOutcome::Rejected(d) = outcome else {
        panic!("not rejected");
    };
    assert_eq!(d.message, "unsupported protocol version");
}

#[test]
fn unknown_field_fixture_is_ignored() {
    let env = envelope("edge-unknown-field.json");
    assert_eq!(env.id, Some(MessageId::String("cmd-y".into())));
    assert!(matches!(
        CommandDecoder::new(false).decode(&env, &EngineState::empty()),
        CommandOutcome::Engine(timetug_engine::command::EngineCommand::Refresh)
    ));
}

#[test]
fn settings_patch_fixtures() {
    let patch: WireSettingsPatch =
        serde_json::from_value(payload_of("edge-settings-patch-full.json")).unwrap();
    assert!(
        patch.lead_time == Some(60.0)
            && patch.enabled == Some(false)
            && patch.system_calendar_enabled == Some(false)
    );
    let mut settings = EngineSettings::default();
    patch.apply_to(&mut settings);
    assert_eq!(
        settings.takeover.hidden_calendar_keys,
        ["a/2".to_string()].into()
    );
    assert!(!settings.system_calendar_enabled);

    let mut untouched = EngineSettings::default();
    let empty: WireSettingsPatch =
        serde_json::from_value(payload_of("edge-settings-patch-empty.json")).unwrap();
    empty.apply_to(&mut untouched);
    assert_eq!(untouched, EngineSettings::default());
}

#[test]
fn reply_error_fixtures_cover_every_kind() {
    for kind in ReplyErrorKind::ALL {
        let parsed = WireReply::parse(&text(&format!("edge-reply-error-{}.json", kind.as_str())));
        assert_eq!(
            parsed,
            WireReply::Error {
                kind,
                message: format!("{} example", kind.as_str())
            }
        );
    }
}

#[test]
fn service_requests_decode_into_their_dtos() {
    fn request<T: DeserializeOwned>(kind: &str) -> T {
        serde_json::from_value(payload_of(&format!("{kind}.json"))).unwrap()
    }
    let http: HttpSendRequest = request("service.http.send");
    assert!(http.method == "POST" && http.body.is_some());
    assert_eq!(
        request::<CredentialsSetRequest>("service.credentials.set")
            .secrets
            .len(),
        1
    );
    assert!(
        !request::<CredentialsUpdateRefreshTokenRequest>("service.credentials.updateRefreshToken")
            .token
            .is_empty()
    );
    assert_eq!(
        request::<AuthPromptCredentialsRequest>("service.auth.promptCredentials")
            .fields
            .len(),
        2
    );
    assert_eq!(
        request::<SignInPrepareRequest>("service.signIn.prepare")
            .help
            .and_then(|h| h.link_title)
            .as_deref(),
        Some("Learn more")
    );
    assert!(
        !request::<TextGenerateRequest>("service.text.generate")
            .prompt
            .is_empty()
    );
    // Requests carry a numeric id the host passes back to tt_engine_reply.
    for kind in MessageCatalog::services() {
        assert!(
            matches!(
                envelope(&format!("{kind}.json")).id,
                Some(MessageId::Number(_))
            ),
            "{kind} id"
        );
    }
}

fn result_of(name: &str) -> Value {
    match WireReply::parse(&text(name)) {
        WireReply::Result(value) => value,
        other => panic!("{name}: not a result: {other:?}"),
    }
}

fn reply<T: DeserializeOwned>(name: &str) -> T {
    serde_json::from_value(result_of(name)).unwrap()
}

#[test]
fn service_replies_decode_into_their_dtos() {
    assert_eq!(
        reply::<HttpSendReply>("edge-reply-http-send.json").status,
        200
    );
    assert!(
        reply::<CredentialsGetReply>("edge-reply-credentials-get.json")
            .secrets
            .is_some()
    );
    assert!(
        reply::<CredentialsGetReply>("edge-reply-credentials-get-none.json")
            .secrets
            .is_none()
    );
    assert!(
        reply::<CredentialsSnapshotReply>("edge-reply-credentials-snapshot.json")
            .snapshot
            .is_some()
    );
    assert!(
        reply::<CredentialsUpdateRefreshTokenReply>("edge-reply-credentials-update.json").updated
    );
    assert_eq!(
        reply::<AuthBeginRedirectReply>("edge-reply-auth-begin.json").session_id,
        "s-1"
    );
    assert!(
        reply::<AuthAuthorizeReply>("edge-reply-auth-authorize.json")
            .url
            .starts_with("http://127.0.0.1")
    );
    assert_eq!(
        reply::<AuthPromptCredentialsReply>("edge-reply-auth-prompt.json")
            .values
            .len(),
        2
    );
    assert!(
        reply::<SignInLastValuesReply>("edge-reply-signin-last.json")
            .values
            .is_some()
    );
    assert_eq!(
        reply::<TextAvailabilityReply>("edge-reply-text-availability.json").kind,
        "available"
    );
    assert_eq!(
        reply::<TextGenerateReply>("edge-reply-text-generate.json").text,
        "yes"
    );
}

// MARK: - External tokens (1.8.0)

#[test]
fn token_requests_decode_into_their_dtos() {
    let get: TokenGetRequest =
        serde_json::from_value(payload_of("service.token.get.json")).unwrap();
    assert!(
        get.connection_id == "1"
            && get.account_id == "acct-1"
            && get.email.as_deref() == Some("me@example.com")
    );
    assert!(get.scopes == ["User.Read", "Calendars.Read"] && !get.force_refresh);
    let sign_in: TokenSignInRequest =
        serde_json::from_value(payload_of("service.token.signIn.json")).unwrap();
    assert!(
        sign_in.sign_in_id == "signin-1" && sign_in.account_id.is_none() && sign_in.email.is_none()
    );
    let cancel: TokenCancelSignInRequest =
        serde_json::from_value(payload_of("service.token.cancelSignIn.json")).unwrap();
    assert_eq!(cancel.sign_in_id, "signin-1");
    for kind in [
        "service.token.get",
        "service.token.signIn",
        "service.token.cancelSignIn",
    ] {
        assert!(
            matches!(
                envelope(&format!("{kind}.json")).id,
                Some(MessageId::Number(_))
            ),
            "{kind} id"
        );
    }
}

#[test]
fn token_replies_decode_into_their_dtos() {
    let ok: TokenGetReply = reply("edge-reply-token-ok.json");
    assert!(
        ok.status == "ok"
            && ok.access_token.as_deref() == Some("redacted-example")
            && ok.expires_at.as_deref() == Some("2026-10-09T09:53:20.000Z")
    );
    let interaction: TokenGetReply = reply("edge-reply-token-interaction-required.json");
    assert!(
        interaction.status == "interactionRequired"
            && interaction.reason.as_deref() == Some("signedOut")
            && interaction.access_token.is_none()
    );
    let unavailable: TokenGetReply = reply("edge-reply-token-unavailable.json");
    assert!(
        unavailable.status == "unavailable"
            && unavailable.reason.as_deref() == Some("brokerUnavailable")
    );
    let blocked: TokenSignInReply = reply("edge-reply-token-blocked.json");
    assert!(
        blocked.status == "blocked"
            && blocked.reason.as_deref() == Some("adminConsentRequired")
            && blocked.account_id.is_none()
    );
}

#[test]
fn token_reply_with_unknown_status_parses_as_is() {
    // The engine, not the parser, decides what an unknown status means (it is an incomplete answer there).
    let reply: TokenGetReply =
        serde_json::from_str(r#"{"status":"somethingNew","extra":1}"#).unwrap();
    assert!(reply.status == "somethingNew" && reply.access_token.is_none());
    let sign_in: TokenSignInReply = serde_json::from_str(r#"{"status":"somethingNew"}"#).unwrap();
    assert_eq!(sign_in.status, "somethingNew");
}

#[test]
fn token_reply_error_kinds_parse() {
    for kind in [
        ReplyErrorKind::Network,
        ReplyErrorKind::Cancelled,
        ReplyErrorKind::Failed,
    ] {
        let parsed = WireReply::parse(&text(&format!("edge-reply-error-{}.json", kind.as_str())));
        assert_eq!(
            parsed,
            WireReply::Error {
                kind,
                message: format!("{} example", kind.as_str())
            }
        );
    }
}

#[test]
fn token_types_are_in_the_service_catalog() {
    for kind in [
        "service.token.get",
        "service.token.signIn",
        "service.token.cancelSignIn",
    ] {
        assert!(MessageCatalog::services().contains(&kind));
    }
}

#[test]
fn old_ready_fixture_still_decodes_and_reencodes_identically() {
    let payload = payload_of("ready.json");
    let ready: WireReady = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(ready.capabilities.external_tokens, None);
    let again = serde_json::to_value(&ready).unwrap();
    assert!(conformance::matches(&again, &payload));
    // The absent capability is omitted, not written as null or false, so an old host sees the 1.7.0 shape.
    let capabilities = again["capabilities"].as_object().expect("no capabilities");
    assert!(!capabilities.contains_key("externalTokens"));
}

#[test]
fn ready_with_external_tokens_true_round_trips() {
    let ready: WireReady =
        serde_json::from_value(payload_of("edge-ready-external-tokens.json")).unwrap();
    assert_eq!(ready.capabilities.external_tokens, Some(true));
    let built = WireReady::new(
        "1.8.0",
        "windows",
        WireReadyCapabilities::with_external_tokens(false, false, Some(true)),
    );
    assert_eq!(built, ready);
}

#[test]
fn ready_capabilities_keep_the_two_argument_initializer() {
    let old = WireReadyCapabilities::new(true, false);
    assert_eq!(old.external_tokens, None);
}

// MARK: - Rust-only tests

/// The reply fixtures (`{"v":1,"result":...}`) and the DTO each result decodes into.
fn reply_round_trip(name: &str) -> Option<RoundTrip> {
    Some(match name {
        "edge-reply-auth-authorize.json" => round_trip::<AuthAuthorizeReply>,
        "edge-reply-auth-begin.json" => round_trip::<AuthBeginRedirectReply>,
        "edge-reply-auth-prompt.json" => round_trip::<AuthPromptCredentialsReply>,
        "edge-reply-credentials-get-none.json" | "edge-reply-credentials-get.json" => {
            round_trip::<CredentialsGetReply>
        }
        "edge-reply-credentials-snapshot.json" => round_trip::<CredentialsSnapshotReply>,
        "edge-reply-credentials-update.json" => round_trip::<CredentialsUpdateRefreshTokenReply>,
        "edge-reply-http-send.json" => round_trip::<HttpSendReply>,
        "edge-reply-signin-last.json" => round_trip::<SignInLastValuesReply>,
        "edge-reply-text-availability.json" => round_trip::<TextAvailabilityReply>,
        "edge-reply-text-generate.json" => round_trip::<TextGenerateReply>,
        "edge-reply-token-ok.json"
        | "edge-reply-token-interaction-required.json"
        | "edge-reply-token-unavailable.json" => round_trip::<TokenGetReply>,
        "edge-reply-token-blocked.json" => round_trip::<TokenSignInReply>,
        _ => return None,
    })
}

/// Every golden fixture (74 at the baseline) is accounted for and round-trips to an equal JSON value: an envelope file
/// through `Envelope` and, where the catalog gives a wire type, through that type; a reply file through `WireReply`
/// and the DTO of its result.
#[test]
fn all_golden_fixtures_round_trip_to_equal_json_values() {
    let files = conformance::golden_files();
    let table = payload_types();
    let (mut envelopes, mut replies, mut typed) = (0, 0, 0);
    for name in &files {
        let fixture = golden_value(name);
        if fixture.get("type").is_some() {
            let env = Envelope::from_value(&fixture).unwrap_or_else(|e| panic!("{name}: {e}"));
            let again = env.to_value();
            if name == "edge-unknown-field.json" {
                // The unknown member is dropped on decode; everything else is kept.
                assert!(crate::compat::is_superset(&fixture, &again), "{name}");
            } else {
                assert!(conformance::matches(&again, &fixture), "{name}");
            }
            let stem = &name[..name.len() - 5];
            if let (Some(payload), Some((_, entry))) = (
                env.payload.as_ref(),
                table.iter().find(|(p, _)| {
                    if p.ends_with('-') {
                        stem.starts_with(p)
                    } else {
                        stem == *p
                    }
                }),
            ) {
                let back = entry(payload).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!(conformance::matches(&back, payload), "{name}");
                typed += 1;
            }
            envelopes += 1;
        } else {
            match WireReply::parse(&golden_text(name)) {
                WireReply::Result(result) => {
                    let entry = reply_round_trip(name).unwrap_or_else(|| panic!("{name}: no DTO"));
                    let back = entry(&result).unwrap_or_else(|e| panic!("{name}: {e}"));
                    assert!(conformance::matches(&back, &result), "{name}");
                    typed += 1;
                }
                WireReply::Error { kind, message } => {
                    assert!(name.starts_with("edge-reply-error-"), "{name}");
                    assert_eq!(message, format!("{} example", kind.as_str()), "{name}");
                    let echoed =
                        json!({"v": 1, "error": {"kind": kind.as_str(), "message": message}});
                    assert!(conformance::matches(&echoed, &fixture), "{name}");
                }
            }
            replies += 1;
        }
    }
    assert_eq!(envelopes + replies, files.len());
    if !conformance::is_baseline() {
        assert_eq!(files.len(), 74);
        assert_eq!((envelopes, replies), (54, 20));
        assert_eq!(typed, 40);
    }
}

#[test]
fn the_catalog_lists_forty_four_distinct_types() {
    let all = MessageCatalog::all();
    assert_eq!(
        (
            MessageCatalog::HOST_TO_ENGINE.len(),
            MessageCatalog::ENGINE_TO_HOST.len(),
            MessageCatalog::services().len(),
            all.len()
        ),
        (20, 7, 17, 44)
    );
    assert_eq!(all.iter().collect::<BTreeSet<_>>().len(), 44);
}
