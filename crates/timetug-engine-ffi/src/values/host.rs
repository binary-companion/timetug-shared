//! Mirrors of the values that cross to and from the host interfaces: HTTP, external tokens and text generation.
#![allow(missing_docs)]

use std::collections::HashMap;

use calendar_core::external_tokens as tokens;
use calendar_core::http;
use calendar_core::url_string::UrlString;
use jiff::Timestamp;
use timetug_engine::inference::text_generator::TextGeneratorAvailability as EngineAvailability;

use super::state::EngineInfo;
use super::{ToEngine, ToMirror, mirror_record};

mirror_record!(HttpRequest <=> http::HttpRequest {
    url: UrlString,
    method: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
});

/// An HTTP answer; the engine lower-cases the header names on the way in, as the Swift type does.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HttpResponse {
    pub status: i64,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
impl ToMirror for http::HttpResponse {
    type Mirror = HttpResponse;
    fn to_mirror(self) -> HttpResponse {
        HttpResponse {
            status: self.status,
            headers: self.headers.to_mirror(),
            body: self.body,
        }
    }
}
impl ToEngine for HttpResponse {
    type Engine = http::HttpResponse;
    fn to_engine(self) -> http::HttpResponse {
        http::HttpResponse::new(self.status, self.headers.to_engine(), self.body)
    }
}

mirror_record!(ExternalAccount <=> tokens::ExternalAccount {
    id: String,
    email: Option<String>,
    connection_id: Option<String>,
});
mirror_record!(ExternalAccessToken <=> tokens::ExternalAccessToken {
    access_token: String,
    expires_at: Timestamp,
});
mirror_record!(ExternalSignInRequest <=> tokens::ExternalSignInRequest {
    account: Option<ExternalAccount>,
    email: Option<String>,
    scopes: Vec<String>,
});
mirror_record!(ExternalSignInResult <=> tokens::ExternalSignInResult {
    account: ExternalAccount,
    token: ExternalAccessToken,
});

/// Whether the host's text model can be used.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum TextGeneratorAvailability {
    Available(EngineInfo),
    Unavailable { reason: String },
    TemporarilyUnavailable { reason: String },
}
impl ToEngine for TextGeneratorAvailability {
    type Engine = EngineAvailability;
    fn to_engine(self) -> EngineAvailability {
        match self {
            TextGeneratorAvailability::Available(info) => {
                EngineAvailability::Available(info.to_engine())
            }
            TextGeneratorAvailability::Unavailable { reason } => {
                EngineAvailability::Unavailable { reason }
            }
            TextGeneratorAvailability::TemporarilyUnavailable { reason } => {
                EngineAvailability::TemporarilyUnavailable { reason }
            }
        }
    }
}
impl ToMirror for EngineAvailability {
    type Mirror = TextGeneratorAvailability;
    fn to_mirror(self) -> TextGeneratorAvailability {
        match self {
            EngineAvailability::Available(info) => {
                TextGeneratorAvailability::Available(info.to_mirror())
            }
            EngineAvailability::Unavailable { reason } => {
                TextGeneratorAvailability::Unavailable { reason }
            }
            EngineAvailability::TemporarilyUnavailable { reason } => {
                TextGeneratorAvailability::TemporarilyUnavailable { reason }
            }
        }
    }
}
