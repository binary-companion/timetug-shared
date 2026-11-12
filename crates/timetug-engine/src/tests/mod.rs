#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::field_reassign_with_default
)]

pub(crate) mod support;

mod account_manager_tests;
mod conformance_tests;
mod dedup_state_store_tests;
mod engine_clock_tests;
mod engine_host_tests;
mod engine_inference_tests;
mod engine_lifecycle_tests;
mod engine_refresh_tests;
mod engine_review_fix_tests;
mod engine_settings_command_tests;
mod engine_settings_tests;
mod engine_system_event_tests;
mod engine_takeover_tests;
mod engine_types_tests;
mod engine_wake_tests;
mod file_settings_storage_tests;
mod inference_prompt_vectors;
mod launch_acknowledgement_tests;
mod ledger_store_tests;
mod legacy_fixture_tests;
mod prompt_builder_tests;
mod source_reconciler_tests;
mod source_refresh_coordinator_tests;
mod text_generator_adjudicator_tests;
mod ui_tick_policy_tests;
