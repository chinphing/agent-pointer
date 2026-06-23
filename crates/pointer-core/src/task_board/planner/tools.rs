//! OpenAI native tool schemas for the planner loop.

use serde_json::{json, Value};

pub fn openai_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the public web for facts needed before planning.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query." }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "session_search",
                "description": "Search or scroll prior conversation messages in this app.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "conversation_id": { "type": "string" },
                        "session_id": { "type": "string" },
                        "around_message_id": { "type": "string" },
                        "window": { "type": "integer" },
                        "limit": { "type": "integer" },
                        "role_filter": { "type": "string" },
                        "sort": { "type": "string", "enum": ["newest", "oldest"] }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "task_board_init",
                "description": "Create a new task board when none exists.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string" },
                        "global_context": {},
                        "items": {
                            "description": "Milestone rows (max 20). Optional work_items[] per row."
                        },
                        "expected_total": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["goal"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "task_board_replace",
                "description": "Replace the full task board when scope changed.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string" },
                        "global_context": {},
                        "items": {},
                        "expected_total": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["items"]
                }
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_planner_native_tools() {
        let tools = openai_tools();
        assert_eq!(tools.len(), 4);
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| {
                t.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "web_search",
                "session_search",
                "task_board_init",
                "task_board_replace"
            ]
        );
    }
}
