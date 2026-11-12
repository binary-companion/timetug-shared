//! `Conformance` of `Support.swift`: where the shared conformance files live, and the baseline mode of the fixture
//! tests.

use std::path::PathBuf;

use serde_json::Value;

use crate::compat::is_superset;
use crate::envelope::Envelope;

/// The repository's `Conformance/` directory, found from this crate's manifest directory (`crates/<crate>`; the C#
/// tests read the same files).
pub(crate) fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../Conformance")
        .canonicalize()
        .unwrap()
}

pub(crate) fn current_golden_directory() -> PathBuf {
    root().join("Fixtures").join("golden")
}

/// `TT_BASELINE_FIXTURES=<dir>` points the fixture tests at an earlier release's golden fixtures (the `compat` CI job
/// exports the baseline tag's `Conformance/Fixtures/golden` there): what an older front end sends and expects must
/// still decode and round-trip with this code.
pub(crate) fn baseline_directory() -> Option<PathBuf> {
    let path = std::env::var("TT_BASELINE_FIXTURES").ok()?;
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(path))
}

pub(crate) fn is_baseline() -> bool {
    baseline_directory().is_some()
}

pub(crate) fn golden_directory() -> PathBuf {
    baseline_directory().unwrap_or_else(current_golden_directory)
}

/// The fixtures being tested: the baseline's in baseline mode, else the current ones.
pub(crate) fn golden_files() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(golden_directory())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    names
}

/// One fixture. In baseline mode a name the baseline does not have (a fixture added since) is read from the current
/// set, so tests that name a newer fixture still run; new fixtures are covered by the normal run.
pub(crate) fn golden(name: &str) -> Vec<u8> {
    let file = golden_directory().join(name);
    if is_baseline() && !file.exists() {
        return std::fs::read(current_golden_directory().join(name)).unwrap();
    }
    std::fs::read(file).unwrap()
}

pub(crate) fn golden_text(name: &str) -> String {
    String::from_utf8(golden(name)).unwrap()
}

pub(crate) fn golden_value(name: &str) -> Value {
    serde_json::from_slice(&golden(name)).unwrap()
}

pub(crate) fn envelope(name: &str) -> Envelope {
    Envelope::from_slice(&golden(name)).unwrap()
}

/// The comparison a re-encoded fixture must pass: exact equality for the current fixtures; in baseline mode the
/// re-encoded value may be a superset of the baseline's (fields added, none removed or changed), because a release
/// may add to the contract within a major version.
pub(crate) fn matches(reencoded: &Value, fixture: &Value) -> bool {
    if is_baseline() {
        is_superset(reencoded, fixture)
    } else {
        reencoded == fixture
    }
}
