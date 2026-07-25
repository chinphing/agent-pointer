//! `session_search` tool registration.

use crate::conversation_store::ConversationStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

const SESSION_SEARCH_MD: &str = include_str!("prompts/session_search.md");
const SESSION_SEARCH_DOC_SOURCE: &str = "session_search/prompts/session_search.md";

pub fn register(reg: &ToolRegistry, store: Arc<ConversationStore>) {
    let doc = SESSION_SEARCH_MD.trim();
    let st = store.clone();
    let handler: ToolHandler =
        Arc::new(move |args: Value| -> Result<String> { st.dispatch_search_tool(&args) });

    reg.register(
        ToolEntry::new(
            "session_search",
            SESSION_SEARCH_DOC_SOURCE,
            "low",
            false,
            doc,
            handler,
        )
        .with_schema(json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "FTS5 search query for discovery mode."
                },
                "conversation_id": {
                    "type": "string",
                    "description": "Target conversation for scroll or read mode."
                },
                "session_id": {
                    "type": "string",
                    "description": "Alias for conversation_id."
                },
                "around_message_id": {
                    "type": "string",
                    "description": "Message id anchor for scroll mode."
                },
                "window": {
                    "type": "integer",
                    "description": "Scroll window size (±window messages). Default 5, max 20."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max conversations in browse/discovery. Default 3, max 10."
                },
                "role_filter": {
                    "type": "string",
                    "description": "Comma-separated roles to filter discovery hits (e.g. user,assistant)."
                },
                "sort": {
                    "type": "string",
                    "enum": ["newest", "oldest"],
                    "description": "Sort discovery results by conversation activity."
                }
            }
        })),
    );
}

pub fn plan_includes_session_search(allowed_tool_names: &[String]) -> bool {
    allowed_tool_names.iter().any(|n| n == "session_search")
}
