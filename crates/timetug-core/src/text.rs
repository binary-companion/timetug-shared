//! Text helpers shared by the rules. Task 3 adds `fold` here.

/// Swift's `String.lowercased()`: every scalar is lowercased on its own. Rust's `str::to_lowercase` also applies the
/// final-sigma rule (`ΟΔΥΣΣΕΥΣ` ends in `ς`); Swift does not (`οδυσσευσ`), and `vectors/text/dedup-key.json` pins that.
pub(crate) fn lowercased(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}
