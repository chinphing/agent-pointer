//! Session task board tool (`task_board`), registered as a **sidecar** tool.

use super::{ToolEntry, ToolHandler, ToolPrompt, ToolRegistry};
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const TASK_BOARD_MD: &str = include_str!("prompts/task_board.md");

/// In-memory task board rows keyed by conversation id.
#[derive(Default)]
pub struct TaskBoardStore {
    inner: RwLock<HashMap<String, Value>>,
}

impl TaskBoardStore {
    pub fn snapshot_for_prompt(&self, conversation_id: &str) -> Option<String> {
        let g = self.inner.read();
        let v = g.get(conversation_id)?;
        let items = v.as_array()?;
        if items.is_empty() {
            return None;
        }
        serde_json::to_string_pretty(v)
            .ok()
            .map(|body| format!("[TASK_BOARD]\n{body}"))
    }

    pub fn items_json(&self, conversation_id: &str) -> Value {
        self.inner
            .read()
            .get(conversation_id)
            .cloned()
            .unwrap_or_else(|| json!([]))
    }

    fn apply_inner(&self, conversation_id: &str, method: &str, items: &[Value]) -> Result<Value> {
        let method = method.trim().to_ascii_lowercase();
        let mut g = self.inner.write();
        match method.as_str() {
            "replace" => {
                g.insert(conversation_id.to_string(), json!(items));
                Ok(json!({ "ok": true, "method": "replace", "count": items.len() }))
            }
            "patch" | "" => {
                let entry = g
                    .entry(conversation_id.to_string())
                    .or_insert_with(|| json!([]));
                let arr = entry
                    .as_array_mut()
                    .ok_or_else(|| anyhow!("task_board: corrupt state"))?;
                for item in items {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    if id.is_empty() {
                        continue;
                    }
                    let mut found = false;
                    for existing in arr.iter_mut() {
                        if existing.get("id").and_then(|v| v.as_str()) == Some(id) {
                            *existing = item.clone();
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        arr.push(item.clone());
                    }
                }
                Ok(json!({ "ok": true, "method": "patch", "count": arr.len() }))
            }
            other => Err(anyhow!("task_board: unknown method {other}")),
        }
    }
}

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
        .unwrap_or("patch");
    let items: Vec<Value> = args
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let summary = store.apply_inner(cid, method, &items)?;
    let snap = store.items_json(cid);
    Ok(json!({ "summary": summary, "items": snap }).to_string())
}

pub fn register_all(reg: &ToolRegistry, store: Arc<TaskBoardStore>) {
    let st = store.clone();
    let h: ToolHandler = Arc::new(move |args| execute_task_board(&st, args));
    let doc = TASK_BOARD_MD.trim();
    reg.register(ToolEntry::new_sidecar(
        "task_board",
        "low",
        false,
        doc,
        Some(ToolPrompt {
            system_prompt: doc.to_string(),
        }),
        h,
    ));
}
