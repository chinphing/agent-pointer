//! Merge a patch JSON row into a stored [`BoardItem`] (v4).

use super::model::{
    constraints_text_field, str_field, BoardItem, ItemStatus, RESULT_SNIPPET_MAX_CHARS,
};
use serde_json::Value;

pub struct RowPatchMerge {
    pub row: BoardItem,
    pub warnings: Vec<serde_json::Value>,
}

pub fn merge_row_patch(prev: &BoardItem, patch_v: &Value) -> BoardItem {
    merge_row_patch_with_warnings(prev, patch_v).row
}

pub fn merge_row_patch_with_warnings(prev: &BoardItem, patch_v: &Value) -> RowPatchMerge {
    let mut row = prev.clone();
    let mut warnings = Vec::new();

    for deprecated in [
        "progress",
        "checkpoint",
        "validate_result_delta",
        "validate_results",
        "extract_requirement",
        "extract_result_delta",
        "extract_results",
        "work_item_mode",
        "dynamic_quota",
    ] {
        if patch_v.get(deprecated).is_some() {
            warnings.push(serde_json::json!({
                "code": "v3_field_rejected",
                "field": deprecated,
                "item_id": row.id,
            }));
            log::warn!(
                "task_board: patch row {} sent deprecated field {deprecated}",
                row.id
            );
        }
    }

    if let Some(s) = patch_v.get("status").and_then(|x| x.as_str()) {
        if let Some(st) = ItemStatus::from_str_loose(s) {
            row.status = st;
        }
    }

    if patch_v
        .get("status")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        warnings.push(serde_json::json!({
            "code": "patch_status_required",
            "item_id": row.id,
        }));
        log::warn!("task_board: patch row {} missing required status", row.id);
    }

    if let Some(t) = patch_v
        .get("title")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        row.title = t.to_string();
    }

    if let Some(dep) = patch_v.get("depends_on") {
        if let Some(arr) = dep.as_array() {
            row.depends_on = arr
                .iter()
                .filter_map(|e| e.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(str::to_string)
                .collect();
        }
    }

    if let Some(n) = patch_v.get("retry_count").and_then(|x| x.as_u64()) {
        row.retry_count = n as u32;
    }

    if let Some(s) = str_field(patch_v, "plan") {
        row.plan = Some(s);
    }
    if let Some(s) = str_field(patch_v, "rules") {
        row.rules = Some(s);
    }
    if patch_v.get("constraints").is_some() || patch_v.get("constraint").is_some() {
        row.constraints = constraints_text_field(patch_v, "constraints", "constraint");
    }
    if let Some(s) =
        str_field(patch_v, "done_when").or_else(|| str_field(patch_v, "validate_requirement"))
    {
        row.done_when = Some(s);
    }
    if let Some(s) = str_field(patch_v, "remark") {
        row.remark = Some(s);
    }
    if let Some(s) = str_field(patch_v, "blocked_by") {
        row.blocked_by = Some(s);
    }

    RowPatchMerge { row, warnings }
}

pub fn compact_row_after_done(prev: &BoardItem, row: &mut BoardItem) {
    if row.status != ItemStatus::Done || prev.status == ItemStatus::Done {
        return;
    }
    row.plan = None;
    if let Some(ref mut remark) = row.remark {
        if remark.chars().count() > RESULT_SNIPPET_MAX_CHARS {
            let compact: String = remark.chars().take(RESULT_SNIPPET_MAX_CHARS).collect();
            *remark = format!("{compact}…");
        }
    }
}
