//! Parse tool arguments (`items`, `method`, flat single-row patch).

use serde_json::{Map, Value};

const PATCH_HOST_KEYS: &[&str] = &[
    "method",
    "goal",
    "global_context",
    "globalContext",
    "ids",
    "finding",
    "expected_total",
    "expectedTotal",
    "_conversation_id",
    "_recent_action_tools",
    "_recent_verify_pass",
    "_recent_verify_report",
    "items",
    "meta",
];

const PATCH_ROW_FIELD_KEYS: &[&str] = &[
    "status",
    "title",
    "output",
    "verification",
    "depends_on",
    "dependsOn",
    "retry_count",
    "retryCount",
    "detailed_plan",
    "detailedPlan",
    "blockedBy",
    "blocked_by",
];

pub fn items_array_from_args(args: &Value) -> Option<Vec<Value>> {
    if let Some(raw) = args.get("items") {
        if let Some(arr) = raw.as_array() {
            return Some(arr.clone());
        }
        if let Some(s) = raw.as_str() {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                return v.as_array().cloned();
            }
        }
        return None;
    }
    flat_patch_row_from_args(args).map(|row| vec![row])
}

pub fn board_rows_from_args(args: &Value) -> Vec<Value> {
    items_array_from_args(args).unwrap_or_default()
}

fn flat_patch_row_from_args(args: &Value) -> Option<Value> {
    let obj = args.as_object()?;
    let id = obj
        .get("item_id")
        .or_else(|| obj.get("id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let has_row_field = obj.keys().any(|k| PATCH_ROW_FIELD_KEYS.contains(&k.as_str()));
    if !has_row_field {
        return None;
    }
    let mut row = Map::new();
    row.insert("id".into(), Value::String(id.to_string()));
    for (k, v) in obj {
        if PATCH_HOST_KEYS.contains(&k.as_str()) || k == "item_id" || k == "id" {
            continue;
        }
        row.insert(k.clone(), v.clone());
    }
    Some(Value::Object(row))
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

pub fn expected_total_from_args(args: &Value) -> Option<u32> {
    args.get("expected_total")
        .or_else(|| args.get("expectedTotal"))
        .or_else(|| {
            args.get("meta")
                .and_then(|m| m.get("expected_total").or_else(|| m.get("expectedTotal")))
        })
        .and_then(value_to_u32_loose)
}

fn value_to_u32_loose(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
            .map(|u| u as u32),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
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

    #[test]
    fn parses_flat_item_id_patch_row() {
        let args = serde_json::json!({
            "item_id": "1",
            "status": "done",
            "verification": "微信应用已打开"
        });
        let items = items_array_from_args(&args).expect("flat row");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "1");
        assert_eq!(items[0]["status"], "done");
    }

    #[test]
    fn flat_row_not_used_when_items_present() {
        let args = serde_json::json!({
            "items": [{"id": "a", "status": "done"}],
            "item_id": "ignored",
            "status": "failed"
        });
        let items = items_array_from_args(&args).expect("items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "a");
    }
    #[test]
    fn parses_expected_total_from_number_or_string() {
        let a = serde_json::json!({"expected_total": 21});
        let b = serde_json::json!({"expected_total": "21"});
        let c = serde_json::json!({"meta": {"expectedTotal": "21"}});
        assert_eq!(expected_total_from_args(&a), Some(21));
        assert_eq!(expected_total_from_args(&b), Some(21));
        assert_eq!(expected_total_from_args(&c), Some(21));
    }
}
