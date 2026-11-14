//! The error mappings: `Failure` to the error thrown to the host, and what a host callback throws back.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use calendar_core::failure::Failure;
use calendar_core::source_types::SourceError as EngineSourceError;

use crate::errors::{FfiError, HostError, HostService, SourceError, failure_from_host_error};

fn engine_source_errors() -> Vec<(EngineSourceError, SourceError)> {
    vec![
        (EngineSourceError::AuthExpired, SourceError::AuthExpired),
        (
            EngineSourceError::Network("down".into()),
            SourceError::Network("down".into()),
        ),
        (
            EngineSourceError::RateLimited {
                retry_after: Some(1.5),
            },
            SourceError::RateLimited {
                retry_after: Some(1.5),
            },
        ),
        (
            EngineSourceError::RateLimited { retry_after: None },
            SourceError::RateLimited { retry_after: None },
        ),
        (
            EngineSourceError::Server { status: 503 },
            SourceError::Server { status: 503 },
        ),
        (
            EngineSourceError::InvalidResponse("bad".into()),
            SourceError::InvalidResponse("bad".into()),
        ),
        (
            EngineSourceError::NeedsPermission,
            SourceError::NeedsPermission,
        ),
        (EngineSourceError::NotFound, SourceError::NotFound),
    ]
}

#[test]
fn every_source_error_maps_both_ways() {
    for (engine, mirror) in engine_source_errors() {
        assert_eq!(SourceError::from(engine.clone()), mirror, "{engine:?}");
        assert_eq!(
            EngineSourceError::from(mirror.clone()),
            engine,
            "{mirror:?}"
        );
    }
}

#[test]
fn every_failure_maps_to_the_error_thrown_to_the_host() {
    for (engine, mirror) in engine_source_errors() {
        assert_eq!(
            FfiError::from(Failure::Source(engine)),
            FfiError::Source(mirror)
        );
    }
    assert_eq!(FfiError::from(Failure::Cancelled), FfiError::Cancelled);
    assert_eq!(
        FfiError::from(Failure::Host("boom".into())),
        FfiError::Host {
            detail: "boom".into()
        }
    );
}

#[test]
fn the_error_has_a_readable_description() {
    assert_eq!(FfiError::Cancelled.to_string(), "cancelled");
    assert_eq!(
        FfiError::Host {
            detail: "boom".into()
        }
        .to_string(),
        "boom"
    );
    assert_eq!(
        FfiError::InvalidConfig {
            reason: "no data directory".into()
        }
        .to_string(),
        "invalid configuration: no data directory"
    );
    assert_eq!(
        FfiError::Source(SourceError::AuthExpired).to_string(),
        "authorization expired"
    );
}

#[test]
fn a_host_error_becomes_a_failure_the_way_the_c_interface_reply_errors_do() {
    // (host error, general service, http, external token source)
    let table: Vec<(HostError, Failure, Failure, Failure)> = vec![
        (
            HostError::Network {
                detail: String::new(),
            },
            Failure::Host("network".into()),
            Failure::Source(EngineSourceError::Network("network".into())),
            Failure::Source(EngineSourceError::Network(
                "Windows sign-in could not reach the network".into(),
            )),
        ),
        (
            HostError::Network {
                detail: "offline".into(),
            },
            Failure::Host("network: offline".into()),
            Failure::Source(EngineSourceError::Network("network: offline".into())),
            Failure::Source(EngineSourceError::Network(
                "Windows sign-in could not reach the network".into(),
            )),
        ),
        (
            HostError::Cancelled,
            Failure::Cancelled,
            Failure::Cancelled,
            Failure::Cancelled,
        ),
        (
            HostError::NotFound,
            Failure::Host("notFound".into()),
            Failure::Source(EngineSourceError::Network("notFound".into())),
            Failure::Source(EngineSourceError::InvalidResponse(
                "Windows sign-in is not available".into(),
            )),
        ),
        (
            HostError::Denied,
            Failure::Host("denied".into()),
            Failure::Source(EngineSourceError::Network("denied".into())),
            Failure::Source(EngineSourceError::InvalidResponse(
                "Windows sign-in is not available".into(),
            )),
        ),
        (
            HostError::Failed {
                detail: String::new(),
            },
            Failure::Host("failed".into()),
            Failure::Source(EngineSourceError::Network("failed".into())),
            Failure::Source(EngineSourceError::InvalidResponse(
                "the Windows account service failed".into(),
            )),
        ),
        (
            HostError::Failed {
                detail: "x".into(),
            },
            Failure::Host("failed: x".into()),
            Failure::Source(EngineSourceError::Network("failed: x".into())),
            Failure::Source(EngineSourceError::InvalidResponse(
                "the Windows account service failed".into(),
            )),
        ),
    ];
    for (error, general, http, token) in table {
        assert_eq!(
            failure_from_host_error(error.clone(), HostService::General),
            general,
            "{error:?} general"
        );
        assert_eq!(
            failure_from_host_error(error.clone(), HostService::Http),
            http,
            "{error:?} http"
        );
        assert_eq!(
            failure_from_host_error(error.clone(), HostService::ExternalToken),
            token,
            "{error:?} token"
        );
    }
}

#[test]
fn a_swift_error_that_is_not_a_host_error_arrives_as_a_failed_host_error() {
    let unexpected = uniffi::UnexpectedUniFFICallbackError::new("Boom.x");
    assert_eq!(
        HostError::from(unexpected),
        HostError::Failed {
            detail: "Boom.x".into()
        }
    );
}

#[test]
fn every_write_error_crosses_as_its_mirror() {
    use crate::values::write::{EventField, WriteError as Mirror};
    use calendar_core::write::types::{EventField as Field, WriteError};
    let cases = vec![
        (
            WriteError::Unsupported {
                fields: [Field::Title, Field::Timing].into(),
            },
            Mirror::Unsupported {
                fields: [EventField::Title, EventField::Timing].into(),
            },
        ),
        (
            WriteError::Conflict {
                fields: [Field::Notes].into(),
            },
            Mirror::Conflict {
                fields: [EventField::Notes].into(),
            },
        ),
        (WriteError::NotFound, Mirror::NotFound),
        (
            WriteError::Forbidden(Some("read only".into())),
            Mirror::Forbidden(Some("read only".into())),
        ),
        (WriteError::Forbidden(None), Mirror::Forbidden(None)),
        (
            WriteError::Invalid("end before start".into()),
            Mirror::Invalid("end before start".into()),
        ),
        (
            WriteError::Partial("stopped".into()),
            Mirror::Partial("stopped".into()),
        ),
    ];
    for (engine, mirror) in cases {
        assert_eq!(
            FfiError::from(Failure::Write(engine.clone())),
            FfiError::Write(mirror.clone()),
            "{engine:?}"
        );
        assert_eq!(
            calendar_core::write::types::WriteError::from(mirror),
            engine
        );
    }
}
