//! Port of `ServiceMessages.swift`: the engine-to-host service requests (`service.*`) and their replies. A request is an
//! `Envelope` whose `id` is the `uint64` the host passes back to `tt_engine_reply`; a reply is `{"v":1,"result":...}` or
//! `{"v":1,"error":{"kind","message"}}`. Binary bodies are base64.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::envelope::{ProtocolVersion, value_as_i64};

/// The `service.*` request types the engine sends to the host.
pub enum ServiceMessages {}

impl ServiceMessages {
    /// Every `service.*` request type.
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
    /// The host could not reach the network.
    Network,
    /// The person or the host cancelled the request.
    Cancelled,
    /// The thing asked for does not exist.
    NotFound,
    /// The host refused, for example for lack of permission.
    Denied,
    /// Any other failure; also what an unrecognized kind in a reply becomes.
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
/// The `error` member of a reply, built by hosts that answer with a failure.
pub struct WireReplyError {
    /// A `ReplyErrorKind` raw value.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Human-readable detail; it may be shown in diagnostics, so it must not contain calendar content.
    pub message: Option<String>,
}

impl WireReplyError {
    /// Creates an error of `kind` with an optional `message`.
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
    /// The request succeeded; the value is the reply's `result` (`null` when the reply had none).
    Result(Value),
    /// The request failed.
    Error {
        /// What kind of failure.
        kind: ReplyErrorKind,
        /// The failure message; empty when the reply gave none.
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
/// Payload of `service.credentials.get`, `.remove` and `.snapshot`: names the connection.
pub struct CredentialsKeyRequest {
    #[serde(rename = "connectionID")]
    /// Id of the stored connection whose secrets are meant.
    pub connection_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.credentials.set`: replaces the secrets stored for a connection.
pub struct CredentialsSetRequest {
    #[serde(rename = "connectionID")]
    /// Id of the stored connection.
    pub connection_id: String,
    /// The secrets to store, by name (for example `refresh_token`).
    pub secrets: BTreeMap<String, String>,
}

/// Reply to `get`: `secrets` absent when the connection has none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsGetReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The stored secrets by name.
    pub secrets: Option<BTreeMap<String, String>>,
}

/// `CredentialsSnapshotReply.Snapshot`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsSnapshot {
    /// The stored secrets by name.
    pub secrets: BTreeMap<String, String>,
    /// An opaque revision string (a UUID); the engine passes it back unchanged.
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.credentials.snapshot`: the secrets together with a revision, or `snapshot` absent when the
/// connection has none.
pub struct CredentialsSnapshotReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The secrets and their revision.
    pub snapshot: Option<CredentialsSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.credentials.updateRefreshToken`: replaces the refresh token only if the stored revision is
/// still
/// the one the engine read.
pub struct CredentialsUpdateRefreshTokenRequest {
    #[serde(rename = "connectionID")]
    /// Id of the stored connection.
    pub connection_id: String,
    /// The new refresh token.
    pub token: String,
    #[serde(rename = "expectedRevision")]
    /// The revision from the snapshot the engine based the update on.
    pub expected_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.credentials.updateRefreshToken`.
pub struct CredentialsUpdateRefreshTokenReply {
    /// False when the stored revision no longer matched (nothing was written).
    pub updated: bool,
}

// MARK: - HTTP

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.http.send`: the engine asks the host to perform one HTTP request.
pub struct HttpSendRequest {
    /// HTTP method, for example `POST`.
    pub method: String,
    /// Absolute request URL.
    pub url: String,
    /// Request headers.
    pub headers: BTreeMap<String, String>,
    /// Base64; absent for no body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.http.send`.
pub struct HttpSendReply {
    /// HTTP status code.
    pub status: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Response headers; absent when there are none.
    pub headers: Option<BTreeMap<String, String>>,
    /// Base64; absent for an empty body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

// MARK: - Sign-in

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.auth.beginRedirect`: the host has started a local listener for an OAuth redirect.
pub struct AuthBeginRedirectReply {
    #[serde(rename = "sessionID")]
    /// Id of the redirect session, passed back in `service.auth.authorize` and `service.auth.closeRedirect`.
    pub session_id: String,
    #[serde(rename = "redirectURI")]
    /// The redirect URI to register with the authorization request.
    pub redirect_uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.auth.authorize`: the host opens `url` for the person to sign in and waits for the redirect.
pub struct AuthAuthorizeRequest {
    #[serde(rename = "sessionID")]
    /// The redirect session from `service.auth.beginRedirect`.
    pub session_id: String,
    /// The authorization URL to open.
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.auth.authorize`.
pub struct AuthAuthorizeReply {
    /// The URL the sign-in was redirected to, including the authorization response.
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.auth.closeRedirect`: the host ends a redirect session. The engine ignores the answer.
pub struct AuthCloseRedirectRequest {
    #[serde(rename = "sessionID")]
    /// The redirect session to close.
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// One input of the host's sign-in form.
pub struct WireCredentialField {
    /// Key under which the typed value is returned.
    pub key: String,
    /// Label shown next to the input.
    pub label: String,
    #[serde(rename = "isSecret")]
    /// True when the input holds a secret such as a password and must be masked.
    pub is_secret: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.auth.promptCredentials`: the host shows a form with these inputs.
pub struct AuthPromptCredentialsRequest {
    /// The inputs, in display order.
    pub fields: Vec<WireCredentialField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.auth.promptCredentials`.
pub struct AuthPromptCredentialsReply {
    /// The values the person typed, by field key.
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Help text for the sign-in form, optionally with a link.
pub struct WireCredentialHelp {
    /// The help text.
    pub text: String,
    #[serde(default, rename = "linkTitle", skip_serializing_if = "Option::is_none")]
    /// Title of the link, shown instead of the raw URL.
    pub link_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The link target.
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.signIn.prepare`: arms the host's sign-in form for the next `service.auth.promptCredentials`.
pub struct SignInPrepareRequest {
    /// Title of the form, for example the account type.
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional help to show on the form.
    pub help: Option<WireCredentialHelp>,
    /// Values to prefill, by field key (never secrets).
    pub values: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Error from the previous attempt to show on the form.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.signIn.lastNonSecretValues`.
pub struct SignInLastValuesReply {
    /// Absent when the sign-in failed before a form was submitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<BTreeMap<String, String>>,
}

// MARK: - Text generation

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.text.availability`.
pub struct TextAvailabilityReply {
    /// `available`, `unavailable` or `temporarilyUnavailable`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Why the model is unavailable, for kinds other than `available`.
    pub reason: Option<String>,
    #[serde(default, rename = "engineID", skip_serializing_if = "Option::is_none")]
    /// Id of the model, with kind `available`.
    pub engine_id: Option<String>,
    #[serde(
        default,
        rename = "engineName",
        skip_serializing_if = "Option::is_none"
    )]
    /// Display name of the model, with kind `available`.
    pub engine_name: Option<String>,
    #[serde(
        default,
        rename = "isOnDevice",
        skip_serializing_if = "Option::is_none"
    )]
    /// True when the model runs on the device, with kind `available`; absent counts as false.
    pub is_on_device: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Payload of `service.text.generate`.
pub struct TextGenerateRequest {
    /// Instructions for the model.
    pub instructions: String,
    /// The prompt to answer.
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Reply to `service.text.generate`.
pub struct TextGenerateReply {
    /// The model's answer.
    pub text: String,
}

// MARK: - External tokens

/// `service.token.get`: a silent access token for an account the host's own account service knows (Windows' Web Account
/// Manager). The host never shows UI for this request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenGetRequest {
    #[serde(rename = "connectionID")]
    /// Id of the stored connection the token is for.
    pub connection_id: String,
    #[serde(rename = "accountID")]
    /// The host account service's id for the account.
    pub account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The account's email address, if known, to help the host pick the account.
    pub email: Option<String>,
    /// OAuth scopes the token must cover.
    pub scopes: Vec<String>,
    #[serde(rename = "forceRefresh")]
    /// True to bypass any cached token and get a new one.
    pub force_refresh: bool,
}

/// Reply to `service.token.get`. `status`: `ok` (with `accessToken` and `expiresAt`, the UTC millisecond string),
/// `interactionRequired` or `unavailable`. `reason` is a documented code, never free text. An unknown `status` decodes as
/// is; the engine decides what it means.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenGetReply {
    /// `ok`, `interactionRequired` or `unavailable`.
    pub status: String,
    #[serde(
        default,
        rename = "accessToken",
        skip_serializing_if = "Option::is_none"
    )]
    /// The access token, with status `ok`.
    pub access_token: Option<String>,
    #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    /// When the token expires, as a `WireDates` string, with status `ok`.
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// A documented reason code, never free text.
    pub reason: Option<String>,
}

/// `service.token.signIn`: the host shows its own sign-in dialog. No timeout; ended by the answer or by
/// `service.token.cancelSignIn` with the same `signInID`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSignInRequest {
    #[serde(rename = "signInID")]
    /// Id of this sign-in, used to cancel it with `service.token.cancelSignIn`.
    pub sign_in_id: String,
    /// OAuth scopes the token must cover.
    pub scopes: Vec<String>,
    #[serde(default, rename = "accountID", skip_serializing_if = "Option::is_none")]
    /// The account to sign in again, when one is known.
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// The account's email address, if known.
    pub email: Option<String>,
}

/// Reply to `service.token.signIn`. `status`: `ok` (with `accountID`, `accessToken`, `expiresAt`), `blocked` or
/// `unavailable`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSignInReply {
    /// `ok`, `blocked` or `unavailable`.
    pub status: String,
    #[serde(default, rename = "accountID", skip_serializing_if = "Option::is_none")]
    /// The host account service's id for the account that signed in, with status `ok`.
    pub account_id: Option<String>,
    #[serde(
        default,
        rename = "accessToken",
        skip_serializing_if = "Option::is_none"
    )]
    /// The access token, with status `ok`.
    pub access_token: Option<String>,
    #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    /// When the token expires, as a `WireDates` string, with status `ok`.
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// A documented reason code, never free text.
    pub reason: Option<String>,
}

/// `service.token.cancelSignIn`: the person cancelled in TimeTug; the host closes its dialog if it still can.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCancelSignInRequest {
    #[serde(rename = "signInID")]
    /// Id of the sign-in to cancel, as given in `service.token.signIn`.
    pub sign_in_id: String,
}
