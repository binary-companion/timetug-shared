//! Port of `CalendarInfoTests.swift`. `refreshPreservesCalendarAccountName` needs `CalendarStore` (Task 4) and stays
//! `pending` in `docs/rust-port/test-parity.md` until then.

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
