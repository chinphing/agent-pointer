//! Payload redaction (plan §7.4). Runs in the background pipeline task,
//! never on the agent hot path.

use serde_json::{Map, Value};

/// Keys containing any of these substrings (case-insensitive) are masked.
const SENSITIVE_SUBSTRINGS: &[&str] = &[
    "token",
    "cookie",
    "password",
    "passwd",
    "secret",
    "authorization",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "credential",
    "bearer",
];

pub fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SENSITIVE_SUBSTRINGS.iter().any(|s| k.contains(s))
}

/// Recursively mask sensitive keys and truncate oversized strings.
pub fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(redact_object(map)),
        Value::Array(arr) => Value::Array(arr.into_iter().map(redact_value).collect()),
        Value::String(s) => Value::String(truncate_string(s)),
        other => other,
    }
}

fn redact_object(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(k, v)| {
            if is_sensitive_key(&k) {
                (k, Value::String("[REDACTED]".into()))
            } else {
                (k, redact_value(v))
            }
        })
        .collect()
}

fn truncate_string(s: String) -> String {
    const MAX: usize = 2048;
    if s.len() <= MAX {
        s
    } else {
        let head: String = s.chars().take(512).collect();
        format!("{head}…[truncated {len} bytes]", len = s.len())
    }
}

/// Redact the payload fields of a trace event in place.
pub fn redact_event_in_place(ev: &mut crate::observability::trace::TraceEvent) {
    if let Some(v) = ev.input.take() {
        ev.input = Some(redact_value(v));
    }
    if let Some(v) = ev.output.take() {
        ev.output = Some(redact_value(v));
    }
    if let Some(err) = ev.error.as_mut() {
        err.message = truncate_string(err.message.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masks_sensitive_keys_recursively() {
        let v = json!({
            "command": "ls",
            "env": { "API_TOKEN": "abc", "PATH": "/usr/bin" },
            "list": [{ "password": "x" }]
        });
        let out = redact_value(v);
        assert_eq!(out["env"]["API_TOKEN"], json!("[REDACTED]"));
        assert_eq!(out["env"]["PATH"], json!("/usr/bin"));
        assert_eq!(out["list"][0]["password"], json!("[REDACTED]"));
        assert_eq!(out["command"], json!("ls"));
    }

    #[test]
    fn truncates_long_strings() {
        let v = Value::String("y".repeat(5000));
        let out = redact_value(v);
        let s = out.as_str().unwrap();
        assert!(s.contains("truncated 5000 bytes"));
        assert!(s.len() < 1000);
    }

    #[test]
    fn sensitive_key_detection_is_case_insensitive() {
        assert!(is_sensitive_key("Authorization"));
        assert!(is_sensitive_key("my_api_key"));
        assert!(is_sensitive_key("token_count")); // substring match is intentionally conservative
        assert!(!is_sensitive_key("command"));
    }
}
