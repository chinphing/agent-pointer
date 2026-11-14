//! Restricted JSON pointer used by provider field mapping.
//!
//! Only RFC 6901 pointers made of plain segments are accepted:
//! `""` (whole document) or `/a/b/0` where every segment is
//! `[A-Za-z0-9_.-]+`. No `~` escapes, no wildcards, no filters, no
//! expressions — a config typo can never turn into code.

use anyhow::{bail, Result};
use serde_json::Value;

/// Validate a restricted JSON pointer. Empty string means the whole document.
pub fn validate(path: &str) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    if !path.starts_with('/') {
        bail!("json path `{path}` must start with `/`");
    }
    for segment in path.split('/').skip(1) {
        if segment.is_empty() {
            bail!("json path `{path}` has an empty segment");
        }
        if let Some(bad) = segment
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '.'))
        {
            bail!("json path `{path}` segment `{segment}` contains `{bad}` (allowed: A-Z a-z 0-9 _ - .)");
        }
    }
    Ok(())
}

/// Resolve a restricted pointer. Returns `None` when a segment is missing.
pub fn get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    value.pointer(path)
}

/// Scalar view: strings verbatim, numbers/bools rendered, null and containers dropped.
pub fn as_string(value: &Value, path: &str) -> Option<String> {
    match get(value, path)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_plain_pointers() {
        assert!(validate("").is_ok());
        assert!(validate("/unionId").is_ok());
        assert!(validate("/data/user/0/id").is_ok());
        assert!(validate("/a-b_c.d").is_ok());
    }

    #[test]
    fn rejects_expressions_and_escapes() {
        assert!(validate("unionId").is_err());
        assert!(validate("$.unionId").is_err());
        assert!(validate("/a//b").is_err());
        assert!(validate("/a~0b").is_err());
        assert!(validate("/a[*]").is_err());
        assert!(validate("/a b").is_err());
    }

    #[test]
    fn resolves_nested_and_indexed_segments() {
        let body = json!({"data": {"users": [{"id": "u-1"}]}, "n": 3, "ok": true});
        assert_eq!(as_string(&body, "/data/users/0/id").as_deref(), Some("u-1"));
        assert_eq!(as_string(&body, "/n").as_deref(), Some("3"));
        assert_eq!(as_string(&body, "/ok").as_deref(), Some("true"));
        assert_eq!(as_string(&body, "/missing"), None);
        // Containers are not scalars.
        assert_eq!(as_string(&body, "/data"), None);
        assert!(get(&body, "").is_some());
    }
}
