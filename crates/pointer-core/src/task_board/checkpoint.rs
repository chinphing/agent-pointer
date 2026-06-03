//! Trim-trigger detection for task_board history soft-exclude.

use super::args::board_rows_from_args;
use super::tool::resolve_method_for_call;
use serde_json::Value;

pub fn is_task_board_tool_name(tool_id: &str) -> bool {
    let n = tool_id.trim().to_ascii_lowercase();
    n.starts_with("task_board")
}

fn patch_marks_done(args: &Value) -> bool {
    board_rows_from_args(args).iter().any(|item| {
        item.get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().eq_ignore_ascii_case("done"))
            .unwrap_or(false)
    })
}

fn non_empty_field(item: &Value, key: &str) -> bool {
    item.get(key)
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn patch_has_nonempty_result_field(item: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        match item.get(*key) {
            Some(Value::String(s)) => !s.trim().is_empty(),
            Some(Value::Array(arr)) => arr.iter().any(|e| {
                e.as_str()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
            }),
            _ => false,
        }
    })
}

fn patch_has_validate_results(args: &Value) -> bool {
    board_rows_from_args(args)
        .iter()
        .any(|item| patch_has_nonempty_result_field(item, &["validate_result_delta", "validate_results"]))
}

fn patch_updates_progress(args: &Value) -> bool {
    board_rows_from_args(args).iter().any(|item| {
        non_empty_field(item, "progress") || non_empty_field(item, "checkpoint")
    })
}

/// Patch that records substantive milestone progress (evidence, position, or completion).
fn patch_has_substantive_progress(args: &Value) -> bool {
    patch_marks_done(args) || patch_has_validate_results(args) || patch_updates_progress(args)
}

/// Whether a successful `task_board` call should trigger history trim.
pub fn task_board_call_is_checkpoint(tool_id: &str, args: &Value) -> bool {
    if !is_task_board_tool_name(tool_id) {
        return false;
    }
    match resolve_method_for_call(tool_id, args).as_str() {
        "init" | "replace" | "finalize" => true,
        "patch" | "" => patch_has_substantive_progress(args),
        _ => false,
    }
}
