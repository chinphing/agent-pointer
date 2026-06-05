//! Sidecar tool registration and execution.

use super::store::TaskBoardStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::anyhow;
use serde_json::Value;
use std::sync::Arc;

const TASK_BOARD_MD: &str = include_str!("prompts/task_board.md");
const TASK_BOARD_DOC_SOURCE: &str = "task_board/prompts/task_board.md";
/// Standalone schemas for flat task_board tools (no `method` enum).
const TASK_BOARD_SCHEMA_YAML: &str = include_str!("prompts/task_board.schema.yaml");

pub fn register(reg: &ToolRegistry, store: Arc<TaskBoardStore>) {
    let doc = crate::tools::tool_doc::doc_markdown_without_schema_fence(TASK_BOARD_MD);
    let schemas = crate::tools::tool_doc::load_tools_from_schema_yaml(TASK_BOARD_SCHEMA_YAML)
        .expect("task_board.schema.yaml must be valid");
    let prompt = doc.trim().to_string();

    for (name, schema) in schemas {
        let st = store.clone();
        // Derive method from tool name: task_board_init → init
        let method = name
            .strip_prefix("task_board_")
            .unwrap_or(&name)
            .to_string();

        let handler: ToolHandler = Arc::new(move |args| {
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
            let (body, _reflection) = st.apply(cid, &method, &args)?;
            Ok(body.to_string())
        });

        reg.register(
            ToolEntry::new_sidecar(
                name.clone(),
                TASK_BOARD_DOC_SOURCE,
                "low",
                false,
                prompt.clone(),
                handler,
            )
            .with_schema(schema),
        );
    }
}

/// Resolve method from tool name for checkpoint / logging.
pub fn resolve_method_for_call(tool_id: &str, _args: &Value) -> String {
    tool_id
        .strip_prefix("task_board_")
        .unwrap_or(tool_id)
        .to_string()
}
