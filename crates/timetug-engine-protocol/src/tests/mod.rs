#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::field_reassign_with_default
)]

pub(crate) mod support;

mod command_mapping_tests;
mod compat_tests;
mod envelope_tests;
mod fixture_tests;
mod legacy_data_directory_tests;
mod mapping_tests;
mod robustness_tests;
mod wire_dates_vector_tests;
