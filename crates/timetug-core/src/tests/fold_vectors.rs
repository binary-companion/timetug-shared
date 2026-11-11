//! Replays `vectors/text/fold.json` and `vectors/text/case-compare.json`, made by the Swift code at the baseline.
//!
//! `fold.json` pins `DuplicateRules.normalize` (which is `folding(options: [.caseInsensitive, .diacriticInsensitive],
//! locale: nil)` followed by the letter-and-number filter) for 2,086 strings: hand-written ones, every scalar of six
//! Unicode blocks, the combining marks U+0300 to U+036F on seven letters, and 200 mixed strings. `case-compare.json` pins
//! `caseInsensitiveCompare(..) == .orderedSame` for 1,410 pairs.

use serde_json::Value;

use crate::dedup::rules::DuplicateRules;
use crate::text::{case_insensitive_eq, fold, grapheme_count, graphemes, prefix_graphemes};

const FOLD: &str = include_str!("../../../../vectors/text/fold.json");
const CASE_COMPARE: &str = include_str!("../../../../vectors/text/case-compare.json");

fn report(area: &str, total: usize, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "{area}: {} of {total} cases failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn fold_vector() {
    let file: Value = serde_json::from_str(FOLD).unwrap();
    assert_eq!(file["area"], "text/fold");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2086);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let text = case["input"]["text"].as_str().unwrap();
        let expected = case["expected"]["result"].as_str().unwrap();
        let actual = DuplicateRules::normalize(text);
        if actual != expected {
            failures.push(format!(
                "{name}: {text:?} expected {expected:?}, got {actual:?}"
            ));
        }
    }
    report("text/fold", cases.len(), failures);
}

#[test]
fn case_compare_vector() {
    let file: Value = serde_json::from_str(CASE_COMPARE).unwrap();
    assert_eq!(file["area"], "text/case-compare");
    let cases = file["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1410);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let a = case["input"]["a"].as_str().unwrap();
        let b = case["input"]["b"].as_str().unwrap();
        let expected = case["expected"]["same"].as_bool().unwrap();
        if case_insensitive_eq(a, b) != expected {
            failures.push(format!("{name}: {a:?} vs {b:?} expected {expected}"));
        }
    }
    report("text/case-compare", cases.len(), failures);
}

#[test]
fn fold_basics() {
    assert_eq!(fold("Café"), "cafe");
    assert_eq!(fold("Straße"), "strasse");
    assert_eq!(fold("ΣΤΑΣ"), "στασ");
    assert_eq!(fold("Hello"), "hello");
    // Combining marks on their own disappear; supplementary-plane text is untouched.
    assert_eq!(fold("e\u{301}\u{301}"), "e");
    assert_eq!(fold("😀"), "😀");
}

#[test]
fn graphemes_follow_user_perceived_characters() {
    let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦";
    assert_eq!(grapheme_count(family), 1);
    assert_eq!(grapheme_count("e\u{301}x"), 2);
    assert_eq!(grapheme_count("🇺🇸🇫🇷"), 2);
    assert_eq!(grapheme_count("👍🏽"), 1);
    assert_eq!(grapheme_count("a\r\nb"), 3);
    assert_eq!(graphemes("ab"), ["a", "b"]);
    assert_eq!(prefix_graphemes(&format!("{family}{family}"), 1), family);
    assert_eq!(prefix_graphemes("abc", 10), "abc");
    assert_eq!(prefix_graphemes("abc", 0), "");
}

#[test]
fn normalize_keeps_a_letter_with_its_combining_marks_as_one_character() {
    // Swift filters by `Character`: a cluster is kept when its first scalar is a letter or number.
    assert_eq!(DuplicateRules::normalize("a\u{20DD}b"), "a\u{20DD}b");
    assert_eq!(DuplicateRules::normalize("  -- ??  "), "");
    assert_eq!(
        DuplicateRules::normalize("Stand-up (daily)"),
        "stand up daily"
    );
}
