//! Port of `CalendarInfoColorTests.swift`.

use super::calendar_store_tests::{FakeSource, new_store};
use super::support::date;
use crate::model::calendar_info::CalendarInfo;

#[test]
fn normalized_hex_accepts_valid_forms() {
    let cases = [
        ("#2f7bf5", "#2F7BF5"),
        ("2F7BF5", "#2F7BF5"),
        ("#abc", "#AABBCC"),
        (" #FF0000 ", "#FF0000"),
        ("abc", "#AABBCC"),
    ];
    for (raw, expected) in cases {
        assert_eq!(
            CalendarInfo::normalized_hex(Some(raw)).as_deref(),
            Some(expected),
            "raw: {raw:?}"
        );
    }
}

#[test]
fn normalized_hex_rejects_invalid() {
    for raw in ["zzz", "#12", "#1234567", "", "#GG0000", "  "] {
        assert_eq!(
            CalendarInfo::normalized_hex(Some(raw)),
            None,
            "raw: {raw:?}"
        );
    }
}

#[test]
fn normalized_hex_nil_is_nil() {
    assert_eq!(CalendarInfo::normalized_hex(None), None);
}

#[test]
fn color_hex_defaults_to_nil_and_is_normalized_by_init() {
    assert_eq!(CalendarInfo::new("s", "c", "T").color_hex(), None);
    assert_eq!(
        CalendarInfo::new("s", "c", "T")
            .with_color_hex(Some("#2f7bf5"))
            .color_hex(),
        Some("#2F7BF5")
    );
    assert_eq!(
        CalendarInfo::new("s", "c", "T")
            .with_color_hex(Some("bogus"))
            .color_hex(),
        None
    );
}

#[test]
fn key_ignores_color_but_equality_does_not() {
    let a = CalendarInfo::new("s", "c", "T").with_color_hex(Some("#FF0000"));
    let b = CalendarInfo::new("s", "c", "T").with_color_hex(Some("#00FF00"));
    assert!(a.key() == b.key() && a.id() == "s/c");
    assert_ne!(a, b);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn refresh_preserves_calendar_color() {
    let source = FakeSource::fake();
    let mut info = CalendarInfo::new("fake", "c1", "Work").with_color_hex(Some("#2F7BF5"));
    info.account_name = Some("iCloud".to_string());
    source.set_calendars(vec![info]);
    let store = new_store(sources![source], "UTC");
    let snapshot = store
        .refresh(date("2026-09-18T10:00:00Z"), 60.0, None, None)
        .await;
    assert_eq!(
        snapshot.calendars.first().and_then(|c| c.color_hex()),
        Some("#2F7BF5")
    );
}
