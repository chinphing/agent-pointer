//! `work_items_export` execution tool (Computer only).

use super::export::{export_work_items, ExportRequest};
use crate::task_board::store::TaskBoardStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;

const EXPORT_MD: &str = include_str!("prompts/work_items_export.md");

pub fn register(reg: &ToolRegistry, store: Arc<TaskBoardStore>) {
    let st = store.clone();
    let handler: ToolHandler = Arc::new(move |args: Value| -> Result<String> {
        let store_key = args
            .get("_conversation_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if store_key.is_empty() {
            return Err(anyhow!("work_items_export: missing host store binding"));
        }
        let workspace = args
            .get("_workspace_root")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if workspace.is_empty() {
            return Err(anyhow!("work_items_export: workspace_root unavailable"));
        }
        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("xlsx")
            .to_string();
        let batch_ids: Vec<String> = args
            .get("batch_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::trim).filter(|s| !s.is_empty()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let columns: Vec<String> = args
            .get("columns")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let doc = st.document(store_key);
        let outcome = export_work_items(
            st.work_items.as_ref(),
            &doc,
            workspace,
            ExportRequest {
                campaign_id: store_key.to_string(),
                batch_ids,
                format,
                output_path,
                columns,
            },
        )?;
        Ok(serde_json::to_string(&outcome)?)
    });

    reg.register(
        ToolEntry::new(
            "work_items_export",
            "task_board/work_item/prompts/work_items_export.md",
            "low",
            false,
            EXPORT_MD.trim(),
            handler,
        )
        .with_schema(json!({
            "type": "object",
            "properties": {
                "batch_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Work-item batch milestone ids; default all wi batches."
                },
                "format": {
                    "type": "string",
                    "enum": ["xlsx", "csv", "txt", "jsonl"],
                    "description": "Export format; default xlsx."
                },
                "output_path": {
                    "type": "string",
                    "description": "Optional path relative to workspace."
                },
                "columns": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }
        })),
    );
}
