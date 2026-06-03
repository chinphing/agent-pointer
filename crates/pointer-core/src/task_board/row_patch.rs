//! Merge a patch JSON row into a stored [`BoardItem`] (v3 semantics).

use super::model::{str_field, BoardItem, ItemStatus, RESULT_SNIPPET_MAX_CHARS};
use super::results_append::{
    append_results_incremental, append_warning_to_json, replace_results_from_value,
    AppendWarning,
};
use serde_json::Value;

pub struct RowPatchMerge {
    pub row: BoardItem,
    pub warnings: Vec<serde_json::Value>,
}

pub fn merge_row_patch(prev: &BoardItem, patch_v: &Value) -> BoardItem {
    merge_row_patch_with_warnings(prev, patch_v).row
}

fn push_warning(warnings: &mut Vec<serde_json::Value>, w: &AppendWarning, item_id: &str) {
    warnings.push(append_warning_to_json(w, item_id));
}

fn push_warning_code(warnings: &mut Vec<serde_json::Value>, code: &'static str, item_id: &str) {
    push_warning(
        warnings,
        &AppendWarning {
            code,
            item_id: None,
        },
        item_id,
    );
}

fn merge_delta_field(
    prev: &[String],
    patch_v: &Value,
    delta_key: &str,
    deprecated_keys: &[&str],
    warnings: &mut Vec<serde_json::Value>,
    item_id: &str,
    warn_code: &'static str,
) -> Vec<String> {
    for key in deprecated_keys {
        if patch_v.get(key).is_some() {
            push_warning_code(warnings, warn_code, item_id);
            log::warn!(
                "task_board: patch row {item_id} sent internal field {key}; use {delta_key}"
            );
        }
    }
    let Some(val) = patch_v.get(delta_key) else {
        return prev.to_vec();
    };
    let (list, wrn) = append_results_incremental(prev, Some(val));
    for w in wrn {
        push_warning(warnings, &w, item_id);
    }
    list
}

fn merge_full_results_on_done(
    prev: &[String],
    patch_v: &Value,
    full_key: &str,
    row_status: ItemStatus,
    warnings: &mut Vec<serde_json::Value>,
    item_id: &str,
    not_done_code: &'static str,
) -> Vec<String> {
    if patch_v.get(full_key).is_none() {
        return prev.to_vec();
    }
    if row_status != ItemStatus::Done {
        push_warning_code(warnings, not_done_code, item_id);
        log::warn!(
            "task_board: patch row {item_id} ignored {full_key} until status is done"
        );
        return prev.to_vec();
    }
    replace_results_from_value(patch_v.get(full_key))
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
    if let Some(s) = str_field(patch_v, "progress").or_else(|| str_field(patch_v, "checkpoint")) {
        row.progress = Some(s);
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

    if patch_v
        .get("status")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        push_warning_code(&mut warnings, "patch_status_required", &row.id);
        log::warn!("task_board: patch row {} missing required status", row.id);
    }

    row.validate_results = merge_delta_field(
        &prev.validate_results,
        patch_v,
        "validate_result_delta",
        &["validate_results"],
        &mut warnings,
        &row.id,
        "validate_results_use_delta_field",
    );
    row.validate_results = merge_full_results_on_done(
        &row.validate_results,
        patch_v,
        "validate_results",
        row.status,
        &mut warnings,
        &row.id,
        "validate_results_only_when_done",
    );

    row.extract_results = merge_delta_field(
        &prev.extract_results,
        patch_v,
        "extract_result_delta",
        &["extract_results"],
        &mut warnings,
        &row.id,
        "extract_results_use_delta_field",
    );
    row.extract_results = merge_full_results_on_done(
        &row.extract_results,
        patch_v,
        "extract_results",
        row.status,
        &mut warnings,
        &row.id,
        "extract_results_only_when_done",
    );

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
