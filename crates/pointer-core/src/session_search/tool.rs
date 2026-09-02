//! `session_search` and `session_read` tool registration.

use crate::conversation_store::ConversationStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

const SESSION_RECALL_MD: &str = include_str!("prompts/session_recall.md");
const SESSION_SEARCH_MD: &str = include_str!("prompts/session_search.md");
const SESSION_READ_MD: &str = include_str!("prompts/session_read.md");
/// Shared appendix key so locator rules appear once for both tools.
const SESSION_RECALL_DOC_SOURCE: &str = "session_search/prompts/session_recall.md";

fn family_doc() -> String {
    format!(
        "{}\n\n{}\n\n{}",
        SESSION_RECALL_MD.trim(),
        SESSION_SEARCH_MD.trim(),
        SESSION_READ_MD.trim()
    )
}

pub fn register(reg: &ToolRegistry, store: Arc<ConversationStore>) {
    let search_store = store.clone();
    let search_handler: ToolHandler =
        Arc::new(move |args: Value| -> Result<String> { search_store.dispatch_search_tool(&args) });
    reg.register(
        ToolEntry::new(
            "session_search",
            SESSION_RECALL_DOC_SOURCE,
            "low",
            false,
            &family_doc(),
            search_handler,
        )
        .with_schema(json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "FTS5 query (required)."
                },
                "conversation_id": {
                    "type": "string",
                    "description": "Limit to one past conversation."
                },
                "session_id": {
                    "type": "string",
                    "description": "Alias for conversation_id."
                },
                "agentInstanceId": {
                    "type": "string",
                    "description": "Limit to one lead or child thread."
                },
                "window": {
                    "type": "integer",
                    "description": "± messages around the primary hit. Default 5, max 20."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max conversation groups. Default 3, max 10."
                },
                "role_filter": {
                    "type": "string",
                    "description": "Comma-separated roles (e.g. user,assistant,tool)."
                },
                "tool_name": {
                    "type": "string",
                    "description": "Comma-separated tool names (e.g. terminal)."
                }
            }
        })),
    );

    let read_handler: ToolHandler =
        Arc::new(move |args: Value| -> Result<String> { store.dispatch_read_tool(&args) });
    reg.register(
        ToolEntry::new(
            "session_read",
            SESSION_RECALL_DOC_SOURCE,
            "low",
            false,
            &family_doc(),
            read_handler,
        )
        .with_schema(json!({
            "type": "object",
            "properties": {
                "agentInstanceId": {
                    "type": "string",
                    "description": "Lead or child thread to read."
                },
                "conversation_id": {
                    "type": "string",
                    "description": "Past conversation (alone) or with agentInstanceId."
                },
                "session_id": {
                    "type": "string",
                    "description": "Alias for conversation_id."
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based start in filtered time order. Default 1. Mutually exclusive with around_message_id."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max messages. Default 40, max 80."
                },
                "around_message_id": {
                    "type": "string",
                    "description": "Start reading at this message. Mutually exclusive with offset."
                }
            }
        })),
    );
}

pub fn plan_includes_session_search(allowed_tool_names: &[String]) -> bool {
    allowed_tool_names
        .iter()
        .any(|n| n == "session_search" || n == "session_read")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;
    use crate::tools::ToolRegistry;
    use crate::tools_system_appendix::generate_tools_system_appendix;
    use tempfile::TempDir;

    #[test]
    fn tools_appendix_emits_family_doc_once() {
        let dir = TempDir::new().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let reg = ToolRegistry::new();
        register(&reg, std::sync::Arc::new(store));
        let allow = vec!["session_search".into(), "session_read".into()];
        let appendix = generate_tools_system_appendix(&reg, &allow);
        assert_eq!(appendix.matches("### Session transcripts").count(), 1);
        assert_eq!(appendix.matches("### `session_search`").count(), 1);
        assert_eq!(appendix.matches("### `session_read`").count(), 1);
        assert!(!appendix.contains("Description: ### Session transcripts"));
    }
}
