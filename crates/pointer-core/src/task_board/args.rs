//! Parse tool arguments (`items`, `method`).

use serde_json::Value;

pub fn items_array_from_args(args: &Value) -> Option<Vec<Value>> {
    let raw = args.get("items")?;
    if let Some(arr) = raw.as_array() {
        return Some(arr.clone());
    }
    if let Some(s) = raw.as_str() {
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            return v.as_array().cloned();
        }
    }
    None
}

pub fn board_rows_from_args(args: &Value) -> Vec<Value> {
    items_array_from_args(args).unwrap_or_default()
}

pub fn resolve_method(tool_id: &str, args: &Value) -> String {
    let name = tool_id.trim().to_ascii_lowercase();
    if name.ends_with(":replace") {
        return "replace".into();
    }
    if name.ends_with(":patch") {
        return "patch".into();
    }
    if name.ends_with(":init") {
        return "init".into();
    }
    if name.ends_with(":prune") {
        return "prune".into();
    }
    if name.ends_with(":finalize") {
        return "finalize".into();
    }
    if name.ends_with(":sync_finding") {
        return "sync_finding".into();
    }
    if name.ends_with(":check_deps") {
        return "check_deps".into();
    }
    args.get("method")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "patch".into())
}

pub fn goal_from_args(args: &Value) -> Option<String> {
    args.get("goal")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn prune_ids_from_args(args: &Value) -> Vec<String> {
    args.get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn finding_from_args(args: &Value) -> Option<String> {
    args.get("finding")
        .or_else(|| args.get("text"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn check_item_id_from_args(args: &Value) -> Option<String> {
    args.get("item_id")
        .or_else(|| args.get("id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_string_items() {
        let args = serde_json::json!({
            "items": "[{\"id\":\"a\",\"title\":\"t\",\"status\":\"pending\"}]"
        });
        let items = items_array_from_args(&args).expect("items");
        assert_eq!(items.len(), 1);
    }
}
