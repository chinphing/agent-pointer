//! Shared assistant tool envelope (JSON protocol today; same shape as the retired XML path).

use std::collections::HashMap;

use serde_json::{Map, Number, Value};

/// Parsed assistant turn: optional sidecar tool calls plus one primary tool call.
#[derive(Debug, Clone)]
pub struct ToolEnvelope {
    pub sidecar: Vec<ToolEnvelopeCall>,
    pub primary: ToolEnvelopeCall,
}

/// One tool call inside a [`ToolEnvelope`].
#[derive(Debug, Clone)]
pub struct ToolEnvelopeCall {
    pub name: String,
    pub arguments: HashMap<String, String>,
    pub thoughts: String,
    pub headline: String,
}

/// Coerce string-map tool arguments to a JSON object string for tool handlers.
pub fn envelope_arguments_to_json_string(args: &HashMap<String, String>) -> String {
    let map: Map<String, Value> = args
        .iter()
        .map(|(k, v)| (k.clone(), coerce_string_arg_to_json_value(v)))
        .collect();
    Value::Object(map).to_string()
}

fn coerce_string_arg_to_json_value(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return Value::String(s.to_string());
    }
    if t.starts_with('{') || t.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            return v;
        }
    }
    if t == "true" {
        return Value::Bool(true);
    }
    if t == "false" {
        return Value::Bool(false);
    }
    if t == "null" {
        return Value::Null;
    }
    if let Ok(i) = t.parse::<i64>() {
        return Value::Number(i.into());
    }
    if let Ok(f) = t.parse::<f64>() {
        if let Some(n) = Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn envelope_args_json_coercion() {
        let mut m = HashMap::new();
        m.insert("seconds".into(), "5".into());
        m.insert("flag".into(), "true".into());
        m.insert("label".into(), "hello".into());
        let j = envelope_arguments_to_json_string(&m);
        let v: Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["seconds"], 5);
        assert_eq!(v["flag"], true);
        assert_eq!(v["label"], "hello");
    }

    #[test]
    fn envelope_args_json_array_for_hotkey_keys() {
        let mut m = HashMap::new();
        m.insert("keys".into(), r#"["command", "space"]"#.into());
        let j = envelope_arguments_to_json_string(&m);
        let v: Value = serde_json::from_str(&j).unwrap();
        let arr = v["keys"].as_array().expect("keys must be JSON array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str(), Some("command"));
        assert_eq!(arr[1].as_str(), Some("space"));
    }
}
