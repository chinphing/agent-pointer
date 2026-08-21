//! `memory` tool registration.

use super::store::MemoryStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

const MEMORY_MD: &str = include_str!("prompts/memory_tool.md");
const MEMORY_DOC_SOURCE: &str = "memory/prompts/memory_tool.md";

pub fn register(reg: &ToolRegistry, store: Arc<MemoryStore>) {
    let doc = MEMORY_MD.trim();
    let st = store.clone();
    let handler: ToolHandler = Arc::new(move |args: Value| -> Result<String> {
        let mut args = args;
        if let Ok(user) = crate::storage::load_user_settings() {
            inject_memory_limits(&mut args, user.memory_char_limit, user.user_char_limit);
        }
        let uid = crate::session_user_env::current_session_user_id().unwrap_or_default();
        st.dispatch_tool(&uid, &args)
    });

    reg.register(
        ToolEntry::new("memory", MEMORY_DOC_SOURCE, "low", false, doc, handler)
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "replace", "remove"],
                        "description": "The action to perform."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["memory", "user"],
                        "description": "Which store: memory (agent notes) or user (user profile)."
                    },
                    "content": {
                        "type": "string",
                        "description": "Entry content. Required for add and replace."
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Short unique substring identifying the entry for replace or remove."
                    }
                },
                "required": ["action", "target"]
            }))
            .with_subagent_inheritance(false),
    );
}

/// Inject host limits before dispatch (called from tool pass).
pub fn inject_memory_limits(args: &mut Value, memory_char_limit: u32, user_char_limit: u32) {
    if let Some(map) = args.as_object_mut() {
        map.insert("_memory_char_limit".into(), json!(memory_char_limit));
        map.insert("_user_char_limit".into(), json!(user_char_limit));
    }
}

pub fn plan_includes_memory(allowed_tool_names: &[String]) -> bool {
    allowed_tool_names.iter().any(|n| n == "memory")
}
