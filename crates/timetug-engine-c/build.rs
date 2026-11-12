//! Supplies `TIMETUG_ENGINE_VERSION` (the release workflow sets it from the tag) to `EngineBuild::VERSION`; the
//! development value is `0.0.0-dev`.
#![allow(clippy::print_stdout)]

fn main() {
    let version = std::env::var("TIMETUG_ENGINE_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "0.0.0-dev".to_string());
    println!("cargo:rustc-env=TIMETUG_ENGINE_VERSION={version}");
    println!("cargo:rerun-if-env-changed=TIMETUG_ENGINE_VERSION");
}
