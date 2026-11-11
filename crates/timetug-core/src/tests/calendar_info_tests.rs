//! Port of `CalendarInfoTests.swift`.

use super::calendar_store_tests::{FakeSource, new_store};
use super::support::date;
use crate::model::calendar_info::CalendarInfo;

#[test]
fn calendar_info_account_name_defaults_to_nil() {
    assert_eq!(CalendarInfo::new("s", "c", "T").account_name, None);
}

#[test]
fn calendar_info_key_ignores_account_name() {
    let mut a = CalendarInfo::new("s", "c", "T");
    a.account_name = Some("iCloud".to_string());
    let mut b = CalendarInfo::new("s", "c", "T");
    b.account_name = Some("Google".to_string());
    assert_ne!(a, b);
    assert_eq!(a.key(), b.key());
    assert_eq!(a.key(), "s/c");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn refresh_preserves_calendar_account_name() {
    let source = FakeSource::fake();
    let mut info = CalendarInfo::new("fake", "c1", "Shared");
    info.account_name = Some("iCloud".to_string());
    source.set_calendars(vec![info]);
    let store = new_store(sources![source], "UTC");
    let snapshot = store
        .refresh(date("2026-09-18T10:00:00Z"), 60.0, None, None)
        .await;
    assert_eq!(
        snapshot
            .calendars
            .first()
            .and_then(|c| c.account_name.as_deref()),
        Some("iCloud")
    );
}
