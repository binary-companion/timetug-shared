//! Port of `ServiceMessages.swift`: the engine-to-host service requests (`service.*`) and their replies. A request is an
//! `Envelope` whose `id` is the `uint64` the host passes back to `tt_engine_reply`; a reply is `{"v":1,"result":...}` or
//! `{"v":1,"error":{"kind","message"}}`. Binary bodies are base64.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::{ProtocolVersion, value_as_i64};

pub enum ServiceMessages {}

impl ServiceMessages {
    pub const TYPES: [&'static str; 17] = [
        "service.credentials.get",
        "service.credentials.set",
        "service.credentials.remove",
        "service.credentials.snapshot",
        "service.credentials.updateRefreshToken",
        "service.http.send",
        "service.auth.beginRedirect",
        "service.auth.authorize",
        "service.auth.closeRedirect",
        "service.auth.promptCredentials",
        "service.signIn.prepare",
        "service.signIn.lastNonSecretValues",
        "service.text.availability",
        "service.text.generate",
        "service.token.get",
        "service.token.signIn",
        "service.token.cancelSignIn",
    ];
}

/// `ReplyErrorKind`: `network, cancelled, notFound, denied, failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReplyErrorKind {
    Network,
    Cancelled,
    NotFound,
    Denied,
    Failed,
}

impl ReplyErrorKind {
    /// Every case, in declaration order.
    pub const ALL: [ReplyErrorKind; 5] = [
        ReplyErrorKind::Network,
        ReplyErrorKind::Cancelled,
        ReplyErrorKind::NotFound,
        ReplyErrorKind::Denied,
        ReplyErrorKind::Failed,
    ];

    /// The Swift raw value.
    pub fn as_str(self) -> &'static str {
        match self {
            ReplyErrorKind::Network => "network",
            ReplyErrorKind::Cancelled => "cancelled",
            ReplyErrorKind::NotFound => "notFound",
            ReplyErrorKind::Denied => "denied",
            ReplyErrorKind::Failed => "failed",
        }
    }

    /// `init?(rawValue:)`.
    pub fn from_raw_value(raw: &str) -> Option<ReplyErrorKind> {
        ReplyErrorKind::ALL.into_iter().find(|k| k.as_str() == raw)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireReplyError {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl WireReplyError {
    pub fn new(kind: ReplyErrorKind, message: Option<String>) -> WireReplyError {
        WireReplyError {
            kind: kind.as_str().to_string(),
            message,
        }
    }
}

/// A parsed reply: a result value or an error.
#[derive(Clone, Debug, PartialEq)]
pub enum WireReply {
    Result(Value),
    Error {
        kind: ReplyErrorKind,
        message: String,
    },
}

impl WireReply {
    fn failed(message: &str) -> WireReply {
        WireReply::Error {
            kind: ReplyErrorKind::Failed,
            message: message.to_string(),
        }
    }

    /// Never fails: anything that is not a well-formed v1 reply becomes a `failed` error, so a request is always
    /// resolved. An unknown error kind is `failed`.
    pub fn parse(json: &str) -> WireReply {
        let value: Value = match serde_json::from_str(json) {
            Ok(value @ Value::Object(_)) => value,
            _ => return WireReply::failed("reply is not a JSON object"),
        };
        let supported = value
            .get("v")
            .filter(|v| v.is_number())
            .and_then(value_as_i64)
            .is_some_and(ProtocolVersion::is_supported);
        if !supported {
            return WireReply::failed("unsupported reply version");
        }
        match value.get("error") {
            Some(Value::Object(error)) => {
                let kind = error
                    .get("kind")
                    .and_then(Value::as_str)
                    .and_then(ReplyErrorKind::from_raw_value)
                    .unwrap_or(ReplyErrorKind::Failed);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                WireReply::Error { kind, message }
            }
            Some(_) => WireReply::failed("reply error is not an object"),
            None => WireReply::Result(value.get("result").cloned().unwrap_or(Value::Null)),
        }
    }
}

// MARK: - Credentials

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsKeyRequest {
    #[serde(rename = "connectionID")]
    pub connection_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsSetRequest {
    #[serde(rename = "connectionID")]
    pub connection_id: String,
    pub secrets: BTreeMap<String, String>,
}

/// Reply to `get`: `secrets` absent when the connection has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsGetReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<BTreeMap<String, String>>,
}

/// `CredentialsSnapshotReply.Snapshot`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsSnapshot {
    pub secrets: BTreeMap<String, String>,
    /// An opaque revision string (a UUID); the engine passes it back unchanged.
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsSnapshotReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<CredentialsSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsUpdateRefreshTokenRequest {
    #[serde(rename = "connectionID")]
    pub connection_id: String,
    pub token: String,
    #[serde(rename = "expectedRevision")]
    pub expected_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsUpdateRefreshTokenReply {
    /// False when the stored revision no longer matched (nothing was written).
    pub updated: bool,
}

// MARK: - HTTP

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpSendRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    /// Base64; absent for no body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpSendReply {
    pub status: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    /// Base64; absent for an empty body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

// MARK: - Sign-in

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthBeginRedirectReply {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "redirectURI")]
    pub redirect_uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthAuthorizeRequest {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthAuthorizeReply {
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthCloseRedirectRequest {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCredentialField {
    pub key: String,
    pub label: String,
    #[serde(rename = "isSecret")]
    pub is_secret: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthPromptCredentialsRequest {
    pub fields: Vec<WireCredentialField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthPromptCredentialsReply {
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCredentialHelp {
    pub text: String,
    #[serde(default, rename = "linkTitle", skip_serializing_if = "Option::is_none")]
    pub link_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInPrepareRequest {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<WireCredentialHelp>,
    pub values: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInLastValuesReply {
    /// Absent when the sign-in failed before a form was submitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<BTreeMap<String, String>>,
}

// MARK: - Text generation

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextAvailabilityReply {
    /// `available`, `unavailable` or `temporarilyUnavailable`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, rename = "engineID", skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<String>,
    #[serde(
        default,
        rename = "engineName",
        skip_serializing_if = "Option::is_none"
    )]
    pub engine_name: Option<String>,
    #[serde(
        default,
        rename = "isOnDevice",
        skip_serializing_if = "Option::is_none"
    )]
    pub is_on_device: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextGenerateRequest {
    pub instructions: String,
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextGenerateReply {
    pub text: String,
}

// MARK: - External tokens

/// `service.token.get`: a silent access token for an account the host's own account service knows (Windows' Web Account
/// Manager). The host never shows UI for this request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenGetRequest {
    #[serde(rename = "connectionID")]
    pub connection_id: String,
    #[serde(rename = "accountID")]
    pub account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub scopes: Vec<String>,
    #[serde(rename = "forceRefresh")]
    pub force_refresh: bool,
}

/// Reply to `service.token.get`. `status`: `ok` (with `accessToken` and `expiresAt`, the UTC millisecond string),
/// `interactionRequired` or `unavailable`. `reason` is a documented code, never free text. An unknown `status` decodes as
/// is; the engine decides what it means.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenGetReply {
    pub status: String,
    #[serde(
        default,
        rename = "accessToken",
        skip_serializing_if = "Option::is_none"
    )]
    pub access_token: Option<String>,
    #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `service.token.signIn`: the host shows its own sign-in dialog. No timeout; ended by the answer or by
/// `service.token.cancelSignIn` with the same `signInID`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSignInRequest {
    #[serde(rename = "signInID")]
    pub sign_in_id: String,
    pub scopes: Vec<String>,
    #[serde(default, rename = "accountID", skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// Reply to `service.token.signIn`. `status`: `ok` (with `accountID`, `accessToken`, `expiresAt`), `blocked` or
/// `unavailable`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSignInReply {
    pub status: String,
    #[serde(default, rename = "accountID", skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(
        default,
        rename = "accessToken",
        skip_serializing_if = "Option::is_none"
    )]
    pub access_token: Option<String>,
    #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `service.token.cancelSignIn`: the person cancelled in TimeTug; the host closes its dialog if it still can.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCancelSignInRequest {
    #[serde(rename = "signInID")]
    pub sign_in_id: String,
}
