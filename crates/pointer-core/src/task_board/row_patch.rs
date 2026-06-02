//! Merge a patch JSON row into a stored [`BoardItem`] (v3 append semantics).

use super::model::{
    append_snippets_from_value, str_field, BoardItem, ItemStatus, RESULT_SNIPPET_MAX_CHARS,
};
use serde_json::Value;

pub fn merge_row_patch(prev: &BoardItem, patch_v: &Value) -> BoardItem {
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

    append_snippets_from_value(&mut row.validate_results, patch_v, "validate_results");
    append_snippets_from_value(&mut row.extract_results, patch_v, "extract_results");

    row
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
