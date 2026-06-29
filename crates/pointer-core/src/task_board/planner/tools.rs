//! OpenAI native tool schemas for the planner loop.

use serde_json::{json, Value};

const TASK_BOARD_SCHEMA_YAML: &str = include_str!("../prompts/task_board.schema.yaml");

fn task_board_tool_parameters(tool_name: &str) -> Value {
    crate::tools::tool_doc::load_tools_from_schema_yaml(TASK_BOARD_SCHEMA_YAML)
        .expect("task_board.schema.yaml must be valid")
        .into_iter()
        .find(|(name, _)| name == tool_name)
        .map(|(_, schema)| schema)
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
}

pub fn openai_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "task_board_init",
                "description": "Create a new task board when none exists.",
                "parameters": task_board_tool_parameters("task_board_init")
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "task_board_replace",
                "description": "Replace item_milestones SOP template when board exists — update steps, rules, constraints, plan, or done_when on template rows.",
                "parameters": task_board_tool_parameters("task_board_replace")
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "task_board_abandon",
                "description": "Mark the current running board failed when the user switched to unrelated work.",
                "parameters": task_board_tool_parameters("task_board_finalize")
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
        assert_eq!(tools.len(), 3);
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
            vec!["task_board_init", "task_board_replace", "task_board_abandon"]
        );
    }

    #[test]
    fn task_board_init_schema_declares_array_work_items() {
        let params = task_board_tool_parameters("task_board_init");
        let wi = params
            .get("properties")
            .and_then(|p| p.get("work_items"))
            .expect("work_items property");
        assert_eq!(wi.get("type"), Some(&json!("array")));
        let items = wi.get("items").expect("work_items.items");
        assert_eq!(items.get("type"), Some(&json!("object")));
        let required = items
            .get("required")
            .and_then(|v| v.as_array())
            .expect("work_items.items.required");
        assert!(required.iter().any(|v| v == "title"));
    }
}
