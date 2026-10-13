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

/// An integer, from a number or a numeric string.
pub(crate) fn int(v: &Value, key: &str) -> Option<i64> {
    get(v, key).and_then(as_int)
}

pub(crate) fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub(crate) fn uint(v: &Value, key: &str) -> Option<u64> {
    int(v, key).and_then(|n| u64::try_from(n).ok())
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
}
