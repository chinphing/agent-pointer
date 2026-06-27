//! Dispatch planner tool calls (native only, no sidecar registry).

use crate::models::{ChatMessage, ModelSettings, Role, ToolCall};
use crate::task_board::TaskBoardStore;
use crate::tools::parse_tool_call_arguments;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;

use super::PlannedMethod;

pub struct PlannerToolPassInput<'a> {
    pub store: Arc<TaskBoardStore>,
    pub store_key: &'a str,
    pub settings: &'a ModelSettings,
    pub work_items_enabled: bool,
}

#[derive(Debug)]
pub struct PlannerToolOutcome {
    pub tool_result: String,
    pub planned: Option<PlannedMethod>,
    pub board_len: usize,
}

pub async fn dispatch_planner_tool(
    input: &mut PlannerToolPassInput<'_>,
    tc: &ToolCall,
) -> Result<PlannerToolOutcome> {
    let args = parse_tool_call_arguments(&tc.arguments);
    match tc.name.as_str() {
        "task_board_init" => dispatch_task_board(input, "init", &args).await,
        "task_board_replace" => dispatch_task_board(input, "replace", &args).await,
        other => Err(anyhow!("planner: unknown tool {other}")),
    }
}

async fn dispatch_task_board(
    input: &PlannerToolPassInput<'_>,
    method: &str,
    args: &Value,
) -> Result<PlannerToolOutcome> {
    let doc = input.store.document(input.store_key);
    if method == "init" && !doc.board_is_empty() {
        return Ok(PlannerToolOutcome {
            tool_result: "ERROR: board already exists — use task_board_replace to replan.".into(),
            planned: None,
            board_len: doc.global_milestones.len(),
        });
    }
    let mut bound = args.clone();
    bound["_conversation_id"] = json!(input.store_key);
    bound["_task_board_work_items_enabled"] = json!(input.work_items_enabled);
    bound["_workspace_root"] = json!(input.settings.workspace_root);
    let (body, _reflection) = input.store.apply(input.store_key, method, &bound)?;
    let board_len = body
        .get("board_len")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let planned = match method {
        "init" => Some(PlannedMethod::Init),
        "replace" => Some(PlannedMethod::Replace),
        _ => None,
    };
    let tool_result = serde_json::to_string(&body).unwrap_or_else(|_| body.to_string());
    Ok(PlannerToolOutcome {
        tool_result,
        planned,
        board_len,
    })
}

pub fn append_assistant_tool_calls(history: &mut Vec<ChatMessage>, out_text: &str, tool_calls: &[ToolCall]) {
    history.push(ChatMessage {
        id: format!("planner_a_{}", uuid::Uuid::new_v4().simple()),
        role: Role::Assistant,
        content: out_text.to_string(),
        status: "done".into(),
        created_at: crate::extensions::now_ms(),
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls.to_vec())
        },
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: None,
        agent_instance_id: None,
        agent_name: None,
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    });
}

pub fn append_tool_result(history: &mut Vec<ChatMessage>, tool_call_id: &str, name: &str, output: &str) {
    history.push(ChatMessage {
        id: format!("planner_t_{}", uuid::Uuid::new_v4().simple()),
        role: Role::Tool,
        content: output.to_string(),
        status: "done".into(),
        created_at: crate::extensions::now_ms(),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: None,
        agent_instance_id: None,
        agent_name: None,
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    });
    let _ = name; // name kept for logging at call site
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ModelSettings, ToolCall};
    use crate::task_board::TaskBoardStore;
    use serde_json::json;
    use std::sync::Arc;

    fn planner_tool_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "tc_test".into(),
            name: name.into(),
            arguments: args.to_string(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }
    }

    fn pass_input<'a>(
        store: Arc<TaskBoardStore>,
        store_key: &'a str,
        settings: &'a ModelSettings,
    ) -> PlannerToolPassInput<'a> {
        PlannerToolPassInput {
            store,
            store_key,
            settings,
            work_items_enabled: false,
        }
    }

    #[tokio::test]
    async fn task_board_init_plans_on_empty_board() {
        let store = Arc::new(TaskBoardStore::new());
        let key = "conv-init";
        let settings = ModelSettings::default();
        let mut input = pass_input(store.clone(), key, &settings);
        let tc = planner_tool_call(
            "task_board_init",
            json!({
                "goal": "campaign",
                "items": [{"id": "s1", "title": "Step", "status": "pending"}]
            }),
        );
        let out = dispatch_planner_tool(&mut input, &tc)
            .await
            .expect("init");
        assert_eq!(out.planned, Some(PlannedMethod::Init));
        assert_eq!(out.board_len, 1);
        assert_eq!(store.document(key).meta.goal, "campaign");
    }

    #[tokio::test]
    async fn task_board_init_rejects_when_board_exists() {
        let store = Arc::new(TaskBoardStore::new());
        let key = "conv-exists";
        store
            .apply(key, "init", &json!({ "goal": "g", "items": [] }))
            .expect("seed");
        let settings = ModelSettings::default();
        let mut input = pass_input(store.clone(), key, &settings);
        let tc = planner_tool_call(
            "task_board_init",
            json!({ "goal": "new", "items": [] }),
        );
        let out = dispatch_planner_tool(&mut input, &tc)
            .await
            .expect("reject");
        assert!(out.planned.is_none());
        assert!(out.tool_result.contains("board already exists"));
    }

    #[tokio::test]
    async fn task_board_replace_plans_and_replaces_item_milestones() {
        let store = Arc::new(TaskBoardStore::new());
        let key = "conv-replace";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "campaign",
                    "work_item_mode": "enumerated",
                    "global_milestones": [
                        {"id": "g_exec", "title": "Exec", "status": "in_progress"}
                    ],
                    "item_milestones": [
                        {"id": "m1", "title": "A", "status": "pending"}
                    ]
                }),
            )
            .expect("init");
        let settings = ModelSettings::default();
        let mut input = pass_input(store.clone(), key, &settings);
        let tc = planner_tool_call(
            "task_board_replace",
            json!({
                "item_milestones": [
                    {"id": "m1", "title": "B", "status": "pending"},
                    {"id": "m2", "title": "C", "status": "pending"}
                ]
            }),
        );
        let out = dispatch_planner_tool(&mut input, &tc)
            .await
            .expect("replace");
        assert_eq!(out.planned, Some(PlannedMethod::Replace));
        let doc = store.document(key);
        assert_eq!(doc.item_milestones.len(), 2);
        assert_eq!(doc.item_milestones[0].id, "m1");
        assert_eq!(doc.item_milestones[1].id, "m2");
        assert_eq!(doc.global_milestones[0].id, "g_exec");
    }

    #[tokio::test]
    async fn unknown_tool_is_error() {
        let store = Arc::new(TaskBoardStore::new());
        let settings = ModelSettings::default();
        let mut input = pass_input(store, "conv-unknown", &settings);
        let tc = planner_tool_call("not_a_planner_tool", json!({}));
        let err = dispatch_planner_tool(&mut input, &tc).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("unknown tool"));
    }

    #[test]
    fn append_helpers_extend_history_roles() {
        let mut history = Vec::new();
        let tc = planner_tool_call("task_board_init", json!({ "goal": "g" }));
        append_assistant_tool_calls(&mut history, "planning", std::slice::from_ref(&tc));
        append_tool_result(&mut history, &tc.id, &tc.name, "OK");
        assert_eq!(history.len(), 2);
        assert!(matches!(history[0].role, Role::Assistant));
        assert!(history[0].tool_calls.is_some());
        assert!(matches!(history[1].role, Role::Tool));
        assert_eq!(history[1].tool_call_id.as_deref(), Some("tc_test"));
    }
}
