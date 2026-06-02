//! Merge a patch JSON row into a stored [`BoardItem`] (v3 append semantics).

use super::model::{str_field, BoardItem, ItemStatus, RESULT_SNIPPET_MAX_CHARS};
use super::results_append::{append_results_incremental, append_warning_to_json};
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

    if let Some(s) = patch_v.get("status").and_then(|x| x.as_str()) {
        if let Some(st) = ItemStatus::from_str_loose(s) {
            row.status = st;
        }
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
    if let Some(s) = str_field(patch_v, "checkpoint") {
        row.checkpoint = Some(s);
    }
    if let Some(s) = str_field(patch_v, "validate_requirement") {
        row.validate_requirement = Some(s);
    }
    if let Some(s) = str_field(patch_v, "extract_requirement") {
        row.extract_requirement = Some(s);
    }
    if let Some(s) = str_field(patch_v, "blocked_by") {
        row.blocked_by = Some(s);
    }

    let mut warnings = Vec::new();
    if patch_v.get("validate_results").is_some() {
        let (list, wrn) =
            append_results_incremental(&prev.validate_results, patch_v.get("validate_results"));
        row.validate_results = list;
        for w in wrn {
            warnings.push(append_warning_to_json(&w, &row.id));
        }
    }
    if patch_v.get("extract_results").is_some() {
        let (list, wrn) =
            append_results_incremental(&prev.extract_results, patch_v.get("extract_results"));
        row.extract_results = list;
        for w in wrn {
            warnings.push(serde_json::json!({
                "code": w.code.replace("validate_results", "extract_results"),
                "item_id": row.id,
            }));
        }
    }

    RowPatchMerge { row, warnings }
}

pub fn compact_row_after_done(prev: &BoardItem, row: &mut BoardItem) {
    if row.status != ItemStatus::Done || prev.status == ItemStatus::Done {
        return;
    }
    row.plan = None;
    for entry in row.validate_results.iter_mut() {
        if entry.chars().count() > RESULT_SNIPPET_MAX_CHARS {
            let compact: String = entry.chars().take(RESULT_SNIPPET_MAX_CHARS).collect();
            *entry = format!("{compact}…");
        }
    }
}
