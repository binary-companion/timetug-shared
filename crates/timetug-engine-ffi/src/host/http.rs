//! `HTTPTransport`: the host sends every HTTP request (the engine has no network stack).
use std::sync::Arc;

use async_trait::async_trait;
use calendar_core::failure::Outcome;
use calendar_core::http::{
    HttpRequest as EngineRequest, HttpResponse as EngineResponse, HttpTransport as EngineTransport,
};

use crate::errors::{HostError, HostService, failure_from_host_error};
use crate::values::host::{HttpRequest, HttpResponse};
use crate::values::{ToEngine, ToMirror};

/// Sends HTTP requests.
#[uniffi::export(foreign)]
#[async_trait]
pub trait HttpTransport: Send + Sync {
    /// Returns any HTTP response, including 4xx and 5xx. A transport failure throws `HostError.Network`; a cancelled
    /// request throws `HostError.Cancelled`. A redirect is not followed: the 3xx response is returned.
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HostError>;
}

/// Presents the host's transport to the engine.
pub struct HttpTransportAdapter {
    host: Arc<dyn HttpTransport>,
}

impl HttpTransportAdapter {
    /// Wraps the host's transport.
    pub fn new(host: Arc<dyn HttpTransport>) -> HttpTransportAdapter {
        HttpTransportAdapter { host }
    }
}

#[async_trait]
impl EngineTransport for HttpTransportAdapter {
    async fn send(&self, request: EngineRequest) -> Outcome<EngineResponse> {
        self.host
            .send(request.to_mirror())
            .await
            .map(|response| response.to_engine())
            .map_err(|error| failure_from_host_error(error, HostService::Http))
    }
}
