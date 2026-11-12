//! Tests of `compat::is_superset` (the relation of `AGENTS.md`, "Additive has one exact meaning for JSON").

use serde_json::json;

use crate::compat::is_superset;

#[test]
fn an_object_that_gains_a_member_is_a_superset() {
    assert!(is_superset(&json!({"a": 1, "b": 2}), &json!({"a": 1})));
    assert!(is_superset(
        &json!({"a": {"x": 1, "y": 2}}),
        &json!({"a": {"x": 1}})
    ));
}

#[test]
fn an_object_that_loses_or_changes_a_member_is_not() {
    assert!(!is_superset(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    assert!(!is_superset(&json!({"a": 2}), &json!({"a": 1})));
    assert!(!is_superset(&json!({"a": {}}), &json!({"a": {"x": 1}})));
}

#[test]
fn an_array_keeps_its_length() {
    assert!(!is_superset(&json!([1, 2]), &json!([1])));
    assert!(!is_superset(&json!([1]), &json!([1, 2])));
    assert!(is_superset(&json!([]), &json!([])));
}

#[test]
fn array_elements_compare_under_the_same_rule() {
    assert!(is_superset(
        &json!([{"a": 1, "b": 2}, 3]),
        &json!([{"a": 1}, 3])
    ));
    assert!(!is_superset(&json!([3, {"a": 1}]), &json!([{"a": 1}, 3])));
}

#[test]
fn a_number_that_becomes_a_string_is_not() {
    assert!(!is_superset(&json!({"a": "1"}), &json!({"a": 1})));
    assert!(!is_superset(&json!("1"), &json!(1)));
}

#[test]
fn null_stays_null() {
    assert!(is_superset(&json!({"a": null}), &json!({"a": null})));
    assert!(!is_superset(&json!({"a": 0}), &json!({"a": null})));
    assert!(!is_superset(&json!({}), &json!({"a": null})));
    assert!(!is_superset(&json!({"a": null}), &json!({"a": 0})));
}

#[test]
fn types_never_mix() {
    assert!(!is_superset(&json!(true), &json!(1)));
    assert!(!is_superset(&json!(1), &json!(true)));
    assert!(!is_superset(&json!({}), &json!([])));
    assert!(!is_superset(&json!([]), &json!({})));
    assert!(!is_superset(&json!("a"), &json!({})));
}

#[test]
fn other_values_must_be_equal() {
    assert!(is_superset(&json!("x"), &json!("x")));
    assert!(!is_superset(&json!("x"), &json!("y")));
    assert!(is_superset(&json!(true), &json!(true)));
    assert!(!is_superset(&json!(false), &json!(true)));
}

#[test]
fn numbers_compare_by_value_not_by_spelling() {
    // Swift's JSONValue holds every number as a Double: `60` and `60.0` are the same number.
    assert!(is_superset(&json!(60.0), &json!(60)));
    assert!(is_superset(&json!(60), &json!(60.0)));
    assert!(!is_superset(&json!(60.5), &json!(60)));
    assert!(is_superset(
        &json!(18_446_744_073_709_551_615_u64),
        &json!(18_446_744_073_709_551_615_u64)
    ));
    assert!(!is_superset(
        &json!(18_446_744_073_709_551_615_u64),
        &json!(18_446_744_073_709_551_614_u64)
    ));
}
