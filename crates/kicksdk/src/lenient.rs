//! Reading Kick's JSON the way Kick writes it.
//!
//! Kick is inconsistent: numbers come as strings (`"duration": "1200"`,
//! `followers_count` either way), fields go missing, and a chat event's
//! `data` is a JSON string inside the JSON - occasionally an object already.
//! These read a [`Value`] without failing on any of that; the typed parsers
//! are built on them so one odd field never loses a whole event.

use serde_json::Value;

/// `data` as Kick sent it: a JSON string to decode again, or already a value.
pub(crate) fn decode_twice(data: &Value) -> Value {
    match data {
        Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| data.clone()),
        other => other.clone(),
    }
}

pub(crate) fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !x.is_null())
}

pub(crate) fn string(v: &Value, key: &str) -> Option<String> {
    get(v, key).and_then(Value::as_str).map(str::to_owned)
}

/// The largest integer an `f64` holds exactly: 2^53 - 1.
const EXACT_F64: f64 = 9_007_199_254_740_991.0;

/// A float that is a whole number an `f64` holds exactly (`1.0`, not `1.5`
/// and not `1e30`, which has long lost its low digits).
fn exact_whole(f: f64) -> Option<f64> {
    (f.fract() == 0.0 && f.abs() <= EXACT_F64).then_some(f)
}

/// An integer, from a number or a numeric string. Out of `i64`'s range, or
/// a fraction, is `None`: never a clamped or truncated number.
pub(crate) fn int(v: &Value, key: &str) -> Option<i64> {
    match get(v, key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().and_then(exact_whole).map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A non-negative integer (an id, a count), from a number or a numeric
/// string, over the whole `u64` range. Negative, out of range or a fraction
/// is `None`: an id is right or missing, never a different id.
pub(crate) fn uint(v: &Value, key: &str) -> Option<u64> {
    match get(v, key)? {
        Value::Number(n) => {
            n.as_u64().or_else(|| n.as_f64().and_then(exact_whole).filter(|f| *f >= 0.0).map(|f| f as u64))
        }
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub(crate) fn boolean(v: &Value, key: &str) -> Option<bool> {
    match get(v, key)? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|n| n != 0),
        Value::String(s) => match s.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// An id that may come as a number or a string, as a string.
pub(crate) fn id_string(v: &Value, key: &str) -> Option<String> {
    match get(v, key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    get(v, key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_what_kick_actually_sends() {
        let v = json!({"a": "1200", "b": 7, "c": null, "d": "x", "e": 1.0, "f": "true"});
        assert_eq!(int(&v, "a"), Some(1200));
        assert_eq!(int(&v, "b"), Some(7));
        assert_eq!(int(&v, "c"), None);
        assert_eq!(int(&v, "d"), None);
        assert_eq!(int(&v, "e"), Some(1));
        assert_eq!(boolean(&v, "f"), Some(true));
        assert_eq!(id_string(&v, "b").as_deref(), Some("7"));
        assert_eq!(decode_twice(&json!("{\"x\":1}")), json!({"x": 1}));
        assert_eq!(decode_twice(&json!({"x": 1})), json!({"x": 1}));
        assert_eq!(decode_twice(&json!("not json")), json!("not json"));
    }

    #[test]
    fn ids_are_right_or_missing() {
        let v = json!({
            "big": u64::MAX, "big_s": "18446744073709551615", "over_i64": 9_223_372_036_854_775_808u64,
            "neg": -1, "half": 1.5, "huge": 1e30, "too_big_s": "18446744073709551616"
        });
        assert_eq!(uint(&v, "big"), Some(u64::MAX));
        assert_eq!(uint(&v, "big_s"), Some(u64::MAX));
        assert_eq!(uint(&v, "over_i64"), Some(9_223_372_036_854_775_808));
        assert_eq!(int(&v, "over_i64"), None, "not clamped to i64::MAX");
        assert_eq!(uint(&v, "neg"), None);
        assert_eq!(int(&v, "neg"), Some(-1));
        assert_eq!((int(&v, "half"), uint(&v, "half")), (None, None));
        assert_eq!((int(&v, "huge"), uint(&v, "huge")), (None, None));
        assert_eq!(uint(&v, "too_big_s"), None);
    }
}
