//! The comparison the backward-compatibility rule of `AGENTS.md` uses on JSON.

use serde_json::Value;

/// True when `current` is a superset of `baseline`. "Additive" has one exact meaning for JSON here, copied from
/// `AGENTS.md`: the new value is a superset of the baseline's. Every member of a baseline object is present with an equal
/// value (recursively); objects may gain members; arrays keep their length and compare element by element under the same
/// rule; any other value is equal and of the same JSON type (a number stays a number, `null` stays `null`).
///
/// Numbers are compared as numbers, not by spelling (`60` and `60.0` are equal), as Swift's `JSONValue` held every number
/// as a `Double`; two integers are compared exactly.
pub fn is_superset(current: &Value, baseline: &Value) -> bool {
    match (current, baseline) {
        (Value::Object(mine), Value::Object(theirs)) => theirs
            .iter()
            .all(|(key, value)| mine.get(key).is_some_and(|m| is_superset(m, value))),
        (Value::Array(mine), Value::Array(theirs)) => {
            mine.len() == theirs.len() && mine.iter().zip(theirs).all(|(m, t)| is_superset(m, t))
        }
        (Value::Number(a), Value::Number(b)) => {
            if let (Some(x), Some(y)) = (a.as_i64(), b.as_i64()) {
                return x == y;
            }
            if let (Some(x), Some(y)) = (a.as_u64(), b.as_u64()) {
                return x == y;
            }
            a.as_f64() == b.as_f64()
        }
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        _ => false,
    }
}
