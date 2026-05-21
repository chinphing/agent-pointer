//! Sidecar tool registration and execution.

use super::args::resolve_method;
use super::store::TaskBoardStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const TASK_BOARD_MD: &str = include_str!("prompts/task_board.md");

fn execute_task_board(store: &TaskBoardStore, args: Value) -> Result<String> {
    let cid = args
        .get("_conversation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if cid.is_empty() {
        return Err(anyhow!(
            "task_board: missing host conversation binding (_conversation_id)"
        ));
    }
    let method = args
        .get("method")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "patch".to_string());
    let (body, _reflection) = store.apply(cid, &method, &args)?;
    Ok(body.to_string())
}

pub fn register(reg: &ToolRegistry, store: Arc<TaskBoardStore>) {
    let st = store.clone();
    let h: ToolHandler = Arc::new(move |args| execute_task_board(&st, args));
    let doc = TASK_BOARD_MD.trim();
    reg.register(ToolEntry::new_sidecar(
        "task_board",
        "low",
        false,
        doc,
        h,
    ));
}

/// Resolve method from qualified tool id + args (for checkpoint / logging).
pub fn resolve_method_for_call(tool_id: &str, args: &Value) -> String {
    resolve_method(tool_id, args)
}
