//! The errors that cross the boundary: [`FfiError`] thrown to the host, [`HostError`] thrown by a host callback.
//!
//! Only `FfiError` and `HostError` are UniFFI error enums, because UniFFI renders the cases of an error enum in upper
//! camel case (`.Source`), while the Swift names are lower camel (`.authExpired`). The payload mirrors are plain enums
//! (`SourceError` here, `WriteError` with the value mirrors), which keep the Swift spelling; the Swift package makes them
//! `Error`s by extension (`docs/bindings-spike.md`, section 3).

use calendar_core::failure::Failure;
use calendar_core::source_types::SourceError as EngineSourceError;

use crate::values::write::WriteError;
use crate::values::{ToEngine, ToMirror};

/// Mirror of `calendar_core::source_types::SourceError`: what a connector reports. Same cases and payloads.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SourceError {
    /// The authorization expired; the account needs signing in again.
    AuthExpired,
    /// The network failed.
    Network(String),
    /// The server asked for fewer requests, optionally saying for how many seconds.
    RateLimited {
        /// Seconds to wait, when the server said.
        retry_after: Option<f64>,
    },
    /// The server answered with an error status.
    Server {
        /// The HTTP status.
        status: i64,
    },
    /// The answer could not be read.
    InvalidResponse(String),
    /// The OS or user has not granted access to a local data store.
    NeedsPermission,
    /// The id names nothing the source can find.
    NotFound,
}

impl From<EngineSourceError> for SourceError {
    fn from(error: EngineSourceError) -> SourceError {
        match error {
            EngineSourceError::AuthExpired => SourceError::AuthExpired,
            EngineSourceError::Network(message) => SourceError::Network(message),
            EngineSourceError::RateLimited { retry_after } => {
                SourceError::RateLimited { retry_after }
            }
            EngineSourceError::Server { status } => SourceError::Server { status },
            EngineSourceError::InvalidResponse(message) => SourceError::InvalidResponse(message),
            EngineSourceError::NeedsPermission => SourceError::NeedsPermission,
            EngineSourceError::NotFound => SourceError::NotFound,
        }
    }
}

impl From<SourceError> for EngineSourceError {
    fn from(error: SourceError) -> EngineSourceError {
        match error {
            SourceError::AuthExpired => EngineSourceError::AuthExpired,
            SourceError::Network(message) => EngineSourceError::Network(message),
            SourceError::RateLimited { retry_after } => {
                EngineSourceError::RateLimited { retry_after }
            }
            SourceError::Server { status } => EngineSourceError::Server { status },
            SourceError::InvalidResponse(message) => EngineSourceError::InvalidResponse(message),
            SourceError::NeedsPermission => EngineSourceError::NeedsPermission,
            SourceError::NotFound => EngineSourceError::NotFound,
        }
    }
}

/// The one error every throwing call of the bindings can throw (mirrors `Failure`, plus a rejected configuration).
#[derive(Clone, Debug, PartialEq, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    /// A connector's own error.
    #[error("{}", EngineSourceError::from(.0.clone()))]
    Source(SourceError),
    /// The host cancelled, or the engine did.
    #[error("cancelled")]
    Cancelled,
    /// A write refused or failed.
    #[error("{}", calendar_core::write::types::WriteError::from(.0.clone()))]
    Write(WriteError),
    /// A failure the host reported, as text.
    #[error("{message}")]
    Host {
        /// The host's description.
        message: String,
    },
    /// The configuration or an argument the host passed is not acceptable.
    #[error("invalid configuration: {reason}")]
    InvalidConfig {
        /// What is wrong.
        reason: String,
    },
}

impl From<Failure> for FfiError {
    fn from(failure: Failure) -> FfiError {
        match failure {
            Failure::Source(error) => FfiError::Source(error.into()),
            Failure::Write(error) => FfiError::Write(error.to_mirror()),
            Failure::Cancelled => FfiError::Cancelled,
            Failure::Host(message) => FfiError::Host { message },
        }
    }
}

/// What a host callback may throw: the five kinds of the C interface's reply errors.
#[derive(Clone, Debug, PartialEq, thiserror::Error, uniffi::Error)]
pub enum HostError {
    /// The host could not reach the network.
    #[error("network")]
    Network {
        /// The host's description; may be empty.
        message: String,
    },
    /// The user or the host cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The thing asked for does not exist.
    #[error("notFound")]
    NotFound,
    /// The host refused, for example for lack of permission.
    #[error("denied")]
    Denied,
    /// Any other failure.
    #[error("failed")]
    Failed {
        /// The host's description; may be empty.
        message: String,
    },
}

/// A callback that throws an error that is not a `HostError` (or panics) reaches Rust as this; it becomes a failure.
impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> HostError {
        HostError::Failed {
            message: error.reason,
        }
    }
}

/// Which engine seam a host callback serves; the C interface converts a reply error differently for each
/// (`RemoteHostServices`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostService {
    /// Credentials, sign-in, settings, text generation, system calendar: a host failure carrying its description.
    General,
    /// HTTP: every failure but a cancellation is `SourceError::Network`.
    Http,
    /// The external token source: the host's text is never used (it could carry account data).
    ExternalToken,
}

impl HostError {
    /// `ServiceError.description`: the message, or the kind's raw value when there is none, otherwise `kind: message`.
    fn description(&self) -> String {
        let (kind, message) = match self {
            HostError::Network { message } => ("network", message.as_str()),
            HostError::Cancelled => ("cancelled", ""),
            HostError::NotFound => ("notFound", ""),
            HostError::Denied => ("denied", ""),
            HostError::Failed { message } => ("failed", message.as_str()),
        };
        if message.is_empty() {
            kind.to_string()
        } else {
            format!("{kind}: {message}")
        }
    }
}

/// Converts what a host callback threw into the engine's [`Failure`], exactly as `RemoteHostServices` converts the
/// reply errors of the C interface.
pub fn failure_from_host_error(error: HostError, service: HostService) -> Failure {
    // A `cancelled` reply is a cancellation for every service (`RequestError::Cancelled`).
    if error == HostError::Cancelled {
        return Failure::Cancelled;
    }
    match service {
        HostService::General => Failure::Host(error.description()),
        HostService::Http => Failure::Source(EngineSourceError::Network(error.description())),
        HostService::ExternalToken => match error {
            HostError::Network { .. } => Failure::Source(EngineSourceError::Network(
                "Windows sign-in could not reach the network".to_string(),
            )),
            HostError::NotFound | HostError::Denied => Failure::Source(
                EngineSourceError::InvalidResponse("Windows sign-in is not available".to_string()),
            ),
            HostError::Failed { .. } => Failure::Source(EngineSourceError::InvalidResponse(
                "the Windows account service failed".to_string(),
            )),
            HostError::Cancelled => Failure::Cancelled,
        },
    }
}

impl From<WriteError> for calendar_core::write::types::WriteError {
    fn from(error: WriteError) -> calendar_core::write::types::WriteError {
        error.to_engine()
    }
}

/// What a host callback that may report a connector or write error throws (the system calendar source): the engine's
/// own error type, so the Swift side throws `FfiError.Source(.needsPermission)` or `FfiError.Write(.forbidden(nil))`.
impl From<uniffi::UnexpectedUniFFICallbackError> for FfiError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> FfiError {
        FfiError::Host {
            message: error.reason,
        }
    }
}

impl From<FfiError> for Failure {
    fn from(error: FfiError) -> Failure {
        match error {
            FfiError::Source(error) => Failure::Source(error.into()),
            FfiError::Write(error) => Failure::Write(error.into()),
            FfiError::Cancelled => Failure::Cancelled,
            FfiError::Host { message } => Failure::Host(message),
            FfiError::InvalidConfig { reason } => Failure::Host(reason),
        }
    }
}
