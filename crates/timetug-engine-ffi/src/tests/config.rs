//! `EngineConfig::validate`: every `InvalidConfig` reason, and a good configuration.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::config::{EngineConfig, GoogleClient, MAX_RETENTION_DAYS, MicrosoftClient};
use crate::errors::FfiError;
use crate::values::accounts::Platform;

fn good() -> EngineConfig {
    EngineConfig {
        data_directory: "/tmp/timetug".into(),
        platform: Platform::Macos,
        google_client: Some(GoogleClient {
            id: "g".into(),
            secret: "s".into(),
        }),
        microsoft_client: Some(MicrosoftClient { id: "m".into() }),
        text_generation: true,
        external_tokens: false,
        system_calendar: true,
        time_zone: "Europe/Berlin".into(),
        ical_days_back: 3,
        ical_days_ahead: 7,
    }
}

type Change = Box<dyn Fn(&mut EngineConfig)>;

fn reason(config: EngineConfig) -> String {
    match config.validate().unwrap_err() {
        FfiError::InvalidConfig { reason } => reason,
        other => panic!("not InvalidConfig: {other:?}"),
    }
}

#[test]
fn a_good_configuration_validates_and_carries_the_connector_settings() {
    let valid = good().validate().unwrap();
    assert_eq!(valid.zone.identifier(), "Europe/Berlin");
    assert_eq!(
        valid.connectors.google,
        Some(("g".to_string(), "s".to_string()))
    );
    assert_eq!(valid.connectors.microsoft_client_id.as_deref(), Some("m"));
    assert_eq!(
        (
            valid.connectors.ical_days_back,
            valid.connectors.ical_days_ahead
        ),
        (3, 7)
    );
    assert_eq!(valid.platform, calendar_core::connection::Platform::MACOS);
}

#[test]
fn clients_may_be_left_out() {
    let mut config = good();
    config.google_client = None;
    config.microsoft_client = None;
    let valid = config.validate().unwrap();
    assert_eq!(valid.connectors.google, None);
    assert_eq!(valid.connectors.microsoft_client_id, None);
}

#[test]
fn each_bad_setting_names_its_reason() {
    let cases: Vec<(&str, Change, &str)> = vec![
        (
            "empty data directory",
            Box::new(|c| c.data_directory = "  ".into()),
            "the data directory is empty",
        ),
        (
            "unknown zone",
            Box::new(|c| c.time_zone = "Mars/Olympus".into()),
            "unknown time zone identifier \"Mars/Olympus\"",
        ),
        (
            "google without secret",
            Box::new(|c| {
                c.google_client = Some(GoogleClient {
                    id: "g".into(),
                    secret: String::new(),
                })
            }),
            "the Google client needs an id and a secret",
        ),
        (
            "google without id",
            Box::new(|c| {
                c.google_client = Some(GoogleClient {
                    id: String::new(),
                    secret: "s".into(),
                })
            }),
            "the Google client needs an id and a secret",
        ),
        (
            "microsoft without id",
            Box::new(|c| c.microsoft_client = Some(MicrosoftClient { id: String::new() })),
            "the Microsoft client needs an id",
        ),
        (
            "retention back",
            Box::new(|c| c.ical_days_back = MAX_RETENTION_DAYS + 1),
            "iCalendar retention back is 3651 days; the most is 3650",
        ),
        (
            "retention ahead",
            Box::new(|c| c.ical_days_ahead = u32::MAX),
            "iCalendar retention ahead is 4294967295 days; the most is 3650",
        ),
    ];
    for (name, change, expected) in cases {
        let mut config = good();
        change(&mut config);
        assert_eq!(reason(config), expected, "{name}");
    }
}

#[test]
fn the_retention_limit_itself_is_allowed() {
    let mut config = good();
    config.ical_days_back = MAX_RETENTION_DAYS;
    config.ical_days_ahead = 0;
    assert!(config.validate().is_ok());
}
