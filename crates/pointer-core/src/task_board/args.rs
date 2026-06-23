//! Parse tool arguments (`global_milestones`, `milestones`, `items`, flat patch).

use serde_json::{Map, Value};

const PATCH_HOST_KEYS: &[&str] = &[
    "method",
    "goal",
    "context",
    "constraint",
    "done_when",
    "global_context",
    "globalContext",
    "ids",
    "finding",
    "expected_total",
    "expectedTotal",
    "work_item_mode",
    "dynamic_quota",
    "work_items",
    "work_items_source",
    "_conversation_id",
    "_recent_action_tools",
    "_recent_verify_pass",
    "_recent_verify_report",
    "items",
    "global_milestones",
    "item_milestones",
    "milestones",
    "meta",
];

const PATCH_ROW_FIELD_KEYS: &[&str] = &[
    "status",
    "title",
    "plan",
    "constraint",
    "done_when",
    "validate_requirement",
    "remark",
    "depends_on",
    "retry_count",
    "blocked_by",
    "delivery_format",
];

fn array_from_key(args: &Value, key: &str) -> Option<Vec<Value>> {
    let raw = args.get(key)?;
    if let Some(arr) = raw.as_array() {
        return Some(arr.clone());
    }
    if let Some(obj) = coerce_json_string_value(raw) {
        return obj.as_array().cloned();
    }
    None
}

/// Accept a JSON object or a stringified JSON object (same tolerance as `milestones[]`).
fn object_from_key(args: &Value, key: &str) -> Option<Value> {
    let raw = args.get(key)?;
    if raw.is_object() {
        return Some(raw.clone());
    }
    coerce_json_string_value(raw).filter(|v| v.is_object())
}

fn coerce_json_string_value(raw: &Value) -> Option<Value> {
    if let Some(s) = raw.as_str() {
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            return Some(v);
        }
    }
    None
}

/// Coerce stringified `work_item_delta` / `work_item_claim` on patch args before apply.
pub fn normalize_patch_args(mut args: Value) -> Value {
    if let Value::Object(ref mut map) = args {
        for key in ["work_item_delta", "work_item_claim"] {
            let Some(raw) = map.get(key) else {
                continue;
            };
            if raw.is_object() {
                continue;
            }
            let Some(v) = coerce_json_string_value(raw) else {
                continue;
            };
            if v.is_object() {
                map.insert(key.to_string(), v);
            }
        }
    }
    args
}

pub fn items_array_from_args(args: &Value) -> Option<Vec<Value>> {
    if let Some(rows) = array_from_key(args, "items") {
        return Some(rows);
    }
    flat_patch_row_from_args(args).map(|row| vec![row])
}

/// Init / replace global milestone rows (`global_milestones` or legacy `items` / `board`).
pub fn global_rows_from_args(args: &Value) -> Vec<Value> {
    array_from_key(args, "global_milestones")
        .or_else(|| array_from_key(args, "items"))
        .or_else(|| array_from_key(args, "board"))
        .unwrap_or_default()
}

/// Replace-only: `item_milestones` whole table.
pub fn item_milestones_from_args(args: &Value) -> Vec<Value> {
    array_from_key(args, "item_milestones").unwrap_or_default()
}

/// Patch item SOP rows (`milestones` len=1).
pub fn milestone_patch_rows_from_args(args: &Value) -> Option<Vec<Value>> {
    array_from_key(args, "milestones")
}

/// Patch global rows (`global_milestones` len=1) or legacy `items` for Type1.
pub fn global_patch_rows_from_args(args: &Value) -> Option<Vec<Value>> {
    if let Some(rows) = array_from_key(args, "global_milestones") {
        return Some(rows);
    }
    if args.get("milestones").is_some() {
        return None;
    }
    items_array_from_args(args)
}

pub fn board_rows_from_args(args: &Value) -> Vec<Value> {
    let global = global_rows_from_args(args);
    if !global.is_empty() {
        return global;
    }
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

pub fn context_from_args(args: &Value) -> Option<String> {
    args.get("context")
        .or_else(|| args.get("meta").and_then(|m| m.get("context")))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn constraint_from_args(args: &Value) -> Option<String> {
    str_meta_field(args, "constraint")
}

pub fn done_when_from_args(args: &Value) -> Option<String> {
    str_meta_field(args, "done_when")
}

fn str_meta_field(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .or_else(|| args.get("meta").and_then(|m| m.get(key)))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn work_item_mode_from_args(args: &Value) -> Option<String> {
    args.get("work_item_mode")
        .or_else(|| args.get("meta").and_then(|m| m.get("work_item_mode")))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn dynamic_quota_from_args(args: &Value) -> Option<u32> {
    args.get("dynamic_quota")
        .or_else(|| args.get("meta").and_then(|m| m.get("dynamic_quota")))
        .and_then(value_to_u32_loose)
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

pub fn replace_has_forbidden_scope(args: &Value) -> bool {
    const FORBIDDEN: &[&str] = &[
        "goal",
        "context",
        "constraint",
        "done_when",
        "expected_total",
        "work_item_mode",
        "dynamic_quota",
        "work_items",
        "work_items_source",
        "global_milestones",
        "items",
        "board",
    ];
    FORBIDDEN.iter().any(|k| args.get(k).is_some())
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
    fn global_milestones_patch_takes_priority() {
        let args = serde_json::json!({
            "global_milestones": [{"id": "g_exec", "status": "done"}],
            "items": [{"id": "ignored", "status": "done"}]
        });
        let rows = global_patch_rows_from_args(&args).expect("rows");
        assert_eq!(rows[0]["id"], "g_exec");
    }

    #[test]
    fn milestones_patch_separate_from_global() {
        let args = serde_json::json!({
            "milestones": [{"id": "m1", "status": "done"}]
        });
        assert!(milestone_patch_rows_from_args(&args).is_some());
        assert!(global_patch_rows_from_args(&args).is_none());
    }

    #[test]
    fn normalize_patch_coerces_string_work_item_delta() {
        let args = serde_json::json!({
            "milestones": "[{\"id\":\"m6\",\"status\":\"done\"}]",
            "work_item_delta": "{\"id\":1,\"status\":\"done\",\"result_summary\":\"ok\"}"
        });
        let norm = normalize_patch_args(args);
        assert!(norm.get("work_item_delta").unwrap().is_object());
        assert_eq!(norm["work_item_delta"]["id"].as_i64(), Some(1));
        assert!(milestone_patch_rows_from_args(&norm).is_some());
    }

    #[test]
    fn object_from_key_rejects_non_object_string() {
        let args = serde_json::json!({ "work_item_delta": "[1,2]" });
        assert!(object_from_key(&args, "work_item_delta").is_none());
    }
}
