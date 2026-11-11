#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::field_reassign_with_default
)]

mod adjudication_tests;
mod all_day_dates_tests;
mod baseline_duplicate_resolver;
mod calendar_info_color_tests;
mod calendar_info_tests;
mod day_agenda_tests;
mod day_agenda_vectors;
mod dedup_key_vectors;
mod dedup_state_fixture;
mod dedup_support;
mod dedup_vectors;
mod duplicate_resolver_equivalence_tests;
mod duplicate_resolver_tests;
mod duplicate_rules_tests;
mod fold_vectors;
mod lesson_book_tests;
mod manual_merge_tests;
mod model_tests;
mod parsers_never_panic;
mod resolver_scale;
mod scheduler_tests;
mod settings_legacy_fixture;
mod settings_rules_tests;
mod sources_tests;
mod support;
mod takeover_guard_tests;
mod takeover_ledger_fixture;
mod takeover_ledger_tests;
mod takeover_policy_tests;
mod takeover_settings_removal_tests;
mod takeover_test_support;
mod takeover_vector_support;
mod takeover_vectors;
mod timetug_calendar_event_tests;
mod vector_support;
mod widget_snapshot_tests;
mod widget_snapshot_vectors;
mod widget_timeline_tests;
