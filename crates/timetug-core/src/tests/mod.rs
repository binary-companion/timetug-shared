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
mod settings_legacy_fixture;
mod settings_rules_tests;
mod sources_tests;
mod support;
mod takeover_settings_removal_tests;
mod timetug_calendar_event_tests;
