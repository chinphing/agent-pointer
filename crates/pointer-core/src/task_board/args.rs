//! Parse tool arguments (`global_milestones`, `milestones`, `items`, flat patch).

use crate::task_board::model::constraints_text_field;
use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

const PATCH_HOST_KEYS: &[&str] = &[
    "method",
    "goal",
    "context",
    "constraints",
    "constraint",
    "rules",
    "done_when",
    "global_context",
    "globalContext",
    "ids",
    "expected_total",
    "expectedTotal",
    "work_item_mode",
    "dynamic_quota",
    "_conversation_id",
    "_recent_action_tools",
    "_recent_verify_pass",
    "_recent_verify_report",
    "items",
    "global_milestones",
    "item_milestones",
    "milestones",
    "current_item",
    "work_item_claim",
    "meta",
];

fn trim_id(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Patch args that were removed — reject instead of silently ignoring.
const REMOVED_PATCH_KEYS: &[&str] = &[
    "work_item_id",
    "work_item_status",
    "global_milestones",
    "result_summary",
    "error_message",
];

pub fn reject_removed_patch_fields(args: &Value) -> Result<()> {
    let Some(obj) = args.as_object() else {
        return Ok(());
    };
    for key in REMOVED_PATCH_KEYS {
        if obj.contains_key(*key) {
            return Err(anyhow!(
                "task_board: patch field `{key}` removed; use `current_item` or `milestones`"
            ));
        }
    }
    Ok(())
}

/// Active work_item focus on patch (`current_item` object).
fn patch_work_item_scope(args: &Value) -> Option<Value> {
    object_from_key(args, "current_item")
}

/// Work queue row id for patch binding (`current_item.id`).
pub fn patch_work_item_id_from_args(args: &Value) -> Option<String> {
    patch_work_item_scope(args)?
        .get("id")
        .and_then(|v| v.as_str())
        .and_then(trim_id)
}

/// Legacy `current_item` with status on patch (removed Type2 work queue).
pub fn patch_work_item_direct_from_args(args: &Value) -> Option<Value> {
    let scope = patch_work_item_scope(args)?;
    if scope.get("status").is_some() {
        return Some(scope);
    }
    None
}

const PATCH_ROW_FIELD_KEYS: &[&str] = &[
    "status",
    "title",
    "plan",
    "constraints",
    "constraint",
    "rules",
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

/// Coerce stringified `work_item_claim` on patch args before apply.
pub fn normalize_patch_args(mut args: Value) -> Value {
    if let Value::Object(ref mut map) = args {
        for key in ["work_item_claim"] {
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

fn json_value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Native JSON array only. A quoted JSON string is a type error (`type: array` in schema).
pub fn typed_array_from_keys(args: &Value, keys: &[&str]) -> Result<Vec<Value>> {
    for key in keys {
        let Some(raw) = args.get(*key) else {
            continue;
        };
        if let Some(arr) = raw.as_array() {
            return Ok(arr.clone());
        }
        return Err(anyhow!(
            "task_board: `{key}` must be a JSON array, got {} — do not quote the array as a string",
            json_value_kind(raw)
        ));
    }
    Ok(Vec::new())
}

/// Init / replace rows. Canonical key is `global_milestones` (JSON array).
/// `milestones` / `items` / `board` are same-type aliases (array, not string).
pub fn global_rows_from_args_typed(args: &Value) -> Result<Vec<Value>> {
    typed_array_from_keys(args, &["global_milestones", "milestones", "items", "board"])
}

/// Init / replace global milestone rows (legacy silent path; prefers native arrays).
pub fn global_rows_from_args(args: &Value) -> Vec<Value> {
    global_rows_from_args_typed(args).unwrap_or_default()
}

/// Replace-only: `item_milestones` whole table (removed — use global_milestones wi_* rows).
pub fn item_milestones_from_args(args: &Value) -> Vec<Value> {
    array_from_key(args, "item_milestones").unwrap_or_default()
}

/// Reject removed init fields (legacy work queue / inline seed).
pub fn reject_init_removed_fields(args: &Value) -> Result<()> {
    const REMOVED: &[&str] = &[
        "work_items",
        "work_items_source",
        "item_milestones",
        "loop_item_plan",
        "loop_item_done_when",
        "loop_item_rules",
        "item_plan",
        "item_done_when",
    ];
    for key in REMOVED {
        if args.get(key).is_some() {
            return Err(anyhow!(
                "task_board: init field `{key}` removed; use global_milestones (g_plan / wi_* / g_deliver) and dynamic_quota only"
            ));
        }
    }
    Ok(())
}

/// Unified patch rows — always use **`milestones`** at the tool surface.
pub fn unified_patch_rows_from_args(args: &Value) -> Result<Option<Vec<Value>>> {
    reject_removed_patch_fields(args)?;
    if args.get("milestones").is_some() {
        let rows = typed_array_from_keys(args, &["milestones"])?;
        return Ok(Some(rows));
    }
    Ok(items_array_from_args(args))
}

/// Reject removed work-item patch fields.
pub fn reject_patch_foreign_work_item_fields(args: &Value) -> Result<()> {
    reject_removed_patch_fields(args)?;
    for key in ["work_items", "work_items_source", "item_milestones"] {
        if args.get(key).is_some() {
            return Err(anyhow!(
                "task_board: `{key}` removed; use task_board_patch with milestones[] (wi_* row ids)"
            ));
        }
    }
    if args.get("work_item_claim").is_some() {
        return Err(anyhow!(
            "task_board: work_item_claim removed; use task_board_patch with milestones[]"
        ));
    }
    if patch_work_item_direct_from_args(args).is_some() {
        return Err(anyhow!(
            "task_board: current_item removed; use task_board_patch with milestones[]"
        ));
    }
    Ok(())
}

/// Reject work-item fields on `patch_milestones`.
pub fn reject_patch_milestones_foreign_fields(args: &Value) -> Result<()> {
    reject_patch_foreign_work_item_fields(args)
}

/// Reject milestone fields on `patch_items`.
pub fn reject_patch_items_foreign_fields(args: &Value) -> Result<()> {
    if let Some(rows) = array_from_key(args, "milestones") {
        if !rows.is_empty() {
            return Err(anyhow!(
                "task_board: patch_items cannot use milestones; use task_board_patch_milestones"
            ));
        }
    }
    if args.get("item_id").is_some() {
        return Err(anyhow!(
            "task_board: patch_items cannot use item_id; use task_board_patch_milestones"
        ));
    }
    if items_array_from_args(args).is_some() {
        return Err(anyhow!(
            "task_board: patch_items cannot use legacy items[]; use task_board_patch_milestones"
        ));
    }
    Ok(())
}

pub fn patch_items_has_work(args: &Value) -> bool {
    args.get("work_item_claim").is_some() || patch_work_item_direct_from_args(args).is_some()
}

/// Strip work-item fields for `patch_milestones` when legacy `patch` combined both surfaces.
pub fn args_for_patch_milestones(args: &Value) -> Value {
    let Some(map) = args.as_object() else {
        return args.clone();
    };
    let mut out = map.clone();
    out.remove("work_item_claim");
    if let Some(ci) = out.get("current_item").and_then(|v| v.as_object()) {
        if ci.get("status").is_some() {
            let mut id_only = serde_json::Map::new();
            if let Some(id) = ci.get("id") {
                id_only.insert("id".into(), id.clone());
            }
            out.insert("current_item".into(), Value::Object(id_only));
        }
    }
    Value::Object(out)
}

/// Strip milestone fields for `patch_items` when legacy `patch` combined both surfaces.
pub fn args_for_patch_items(args: &Value) -> Value {
    let Some(map) = args.as_object() else {
        return args.clone();
    };
    let mut out = map.clone();
    out.remove("milestones");
    out.remove("items");
    out.remove("item_id");
    out.remove("global_context");
    Value::Object(out)
}

/// Patch item SOP rows (`milestones` len=1).
pub fn milestone_patch_rows_from_args(args: &Value) -> Option<Vec<Value>> {
    array_from_key(args, "milestones")
}

pub fn board_rows_from_args(args: &Value) -> Vec<Value> {
    let global = global_rows_from_args(args);
    if !global.is_empty() {
        return global;
    }
    if let Some(rows) = milestone_patch_rows_from_args(args) {
        return rows;
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
    let has_row_field = obj
        .keys()
        .any(|k| PATCH_ROW_FIELD_KEYS.contains(&k.as_str()));
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

pub fn constraints_from_args(args: &Value) -> Option<String> {
    constraints_text_field(args, "constraints", "constraint").or_else(|| {
        args.get("meta")
            .and_then(|m| constraints_text_field(m, "constraints", "constraint"))
    })
}

#[deprecated(note = "use constraints_from_args")]
pub fn constraint_from_args(args: &Value) -> Option<String> {
    constraints_from_args(args)
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
        "constraints",
        "rules",
        "done_when",
        "expected_total",
        "work_item_mode",
        "dynamic_quota",
        "work_items",
        "work_items_source",
        "item_milestones",
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
    fn typed_array_rejects_quoted_json_string() {
        let err = typed_array_from_keys(
            &serde_json::json!({
                "milestones": "[{\"id\":\"a\"}]"
            }),
            &["milestones"],
        )
        .expect_err("string is not an array");
        let msg = err.to_string();
        assert!(msg.contains("JSON array"), "{msg}");
        assert!(msg.contains("string"), "{msg}");
    }

    #[test]
    fn milestones_patch_rows_from_args_reads_milestones_array() {
        let args = serde_json::json!({
            "milestones": [{"id": "m1", "status": "done"}]
        });
        let rows = milestone_patch_rows_from_args(&args).expect("rows");
        assert_eq!(rows[0]["id"], "m1");
    }

    #[test]
    fn normalize_patch_coerces_string_work_item_claim() {
        let args = serde_json::json!({
            "work_item_claim": "{\"target_key\":\"t1\",\"title\":\"T1\"}"
        });
        let norm = normalize_patch_args(args);
        assert!(norm.get("work_item_claim").unwrap().is_object());
        assert_eq!(norm["work_item_claim"]["target_key"], "t1");
    }

    #[test]
    fn reject_init_loop_item_plan_field() {
        let args = serde_json::json!({ "goal": "g", "loop_item_plan": "steps" });
        let err = super::reject_init_removed_fields(&args).unwrap_err();
        assert!(err.to_string().contains("loop_item_plan"));
    }

    #[test]
    fn reject_init_work_items_field() {
        let args = serde_json::json!({ "goal": "g", "work_items": [{ "title": "a" }] });
        let err = super::reject_init_removed_fields(&args).unwrap_err();
        assert!(err.to_string().contains("work_items"));
    }

    #[test]
    fn reject_init_work_items_source_field() {
        let args = serde_json::json!({ "goal": "g", "work_items_source": "/tmp/list.json" });
        let err = super::reject_init_removed_fields(&args).unwrap_err();
        assert!(err.to_string().contains("work_items_source"));
    }

    #[test]
    fn patch_work_item_direct_from_current_item() {
        let args = serde_json::json!({
            "current_item": {
                "id": "2",
                "status": "failed",
                "error_message": "timeout"
            }
        });
        let scope = super::patch_work_item_direct_from_args(&args).expect("scope");
        assert_eq!(scope["id"], "2");
        assert_eq!(scope["status"], "failed");
        assert_eq!(scope["error_message"], "timeout");
    }

    #[test]
    fn reject_removed_patch_fields_work_item_id() {
        let args = serde_json::json!({
            "work_item_id": "1",
            "milestones": [{"id": "m1", "status": "done"}]
        });
        let err = super::reject_removed_patch_fields(&args).unwrap_err();
        assert!(err.to_string().contains("work_item_id"));
    }

    #[test]
    fn reject_removed_patch_fields_global_milestones() {
        let args = serde_json::json!({
            "global_milestones": [{"id": "m1", "status": "done"}]
        });
        let err = super::unified_patch_rows_from_args(&args).unwrap_err();
        assert!(err.to_string().contains("global_milestones"));
    }

    #[test]
    fn object_from_key_rejects_non_object_string() {
        let args = serde_json::json!({ "work_item_claim": "[1,2]" });
        assert!(object_from_key(&args, "work_item_claim").is_none());
    }
}
