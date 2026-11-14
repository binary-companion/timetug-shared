//! Mirrors of the account types: stored connections, sign-in forms and the account status.
#![allow(missing_docs)]

use std::collections::HashMap;

use calendar_core::connection as conn;
use calendar_core::connection::Platform as EnginePlatform;
use calendar_core::url_string::UrlString;
use timetug_engine::accounts::account_manager as manager;

use super::{ToEngine, ToMirror, mirror_record, mirror_unit_enum};

/// Swift's `ConnectionID` is a `String`; the mirror is the same.
pub type ConnectionId = String;

mirror_record!(Connection <=> conn::Connection {
    kind_id: String,
    connection_id: ConnectionId,
    display_name: String,
    config: HashMap<String, String>,
});

mirror_record!(CredentialField <=> conn::CredentialField { key: String, label: String, is_secret: bool });

/// How an account kind signs in.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AuthorizationMethod {
    Oauth,
    Password { fields: Vec<CredentialField> },
    System,
}
impl ToMirror for conn::AuthorizationMethod {
    type Mirror = AuthorizationMethod;
    fn to_mirror(self) -> AuthorizationMethod {
        match self {
            conn::AuthorizationMethod::Oauth => AuthorizationMethod::Oauth,
            conn::AuthorizationMethod::Password { fields } => AuthorizationMethod::Password {
                fields: fields.to_mirror(),
            },
            conn::AuthorizationMethod::System => AuthorizationMethod::System,
        }
    }
}
impl ToEngine for AuthorizationMethod {
    type Engine = conn::AuthorizationMethod;
    fn to_engine(self) -> conn::AuthorizationMethod {
        match self {
            AuthorizationMethod::Oauth => conn::AuthorizationMethod::Oauth,
            AuthorizationMethod::Password { fields } => conn::AuthorizationMethod::Password {
                fields: fields.to_engine(),
            },
            AuthorizationMethod::System => conn::AuthorizationMethod::System,
        }
    }
}

mirror_record!(CredentialHelp <=> conn::CredentialHelp {
    text: String,
    link_title: Option<String>,
    url: Option<UrlString>,
});

mirror_record!(CredentialSnapshot <=> conn::CredentialSnapshot {
    secrets: HashMap<String, String>,
    revision: uuid::Uuid,
});

/// The platform a host runs on (one member of the engine's option set). `Android` has no flag in the connector library
/// yet (it has none until the library adds it), so on Android no kind is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Platform {
    Macos,
    Ios,
    Android,
    Linux,
    Windows,
}
impl Platform {
    /// The engine's flag for this platform.
    pub fn to_flag(self) -> EnginePlatform {
        match self {
            Platform::Macos => EnginePlatform::MACOS,
            Platform::Ios => EnginePlatform::IOS,
            // Bit 4: the flag the connector library adds for Android (docs/bindings.md). No kind lists it yet.
            Platform::Android => EnginePlatform::from_raw_value(1 << 4),
            Platform::Linux => EnginePlatform::LINUX,
            Platform::Windows => EnginePlatform::WINDOWS,
        }
    }
}

mirror_unit_enum!(WaitingStyle <=> manager::WaitingStyle { Browser, InApp });

mirror_record!(forward_only AccountStatus <=> manager::Status {
    accounts: Vec<Connection>,
    is_working: bool,
    build_failures: HashMap<ConnectionId, String>,
    error_message: Option<String>,
    waiting_style: WaitingStyle,
});
