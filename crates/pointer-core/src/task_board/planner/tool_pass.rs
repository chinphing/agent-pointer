//! Dispatch planner tool calls (native only, no sidecar registry).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::ConversationLlmStats;
use crate::models::{ChatMessage, ModelSettings, Role, ToolCall};
use crate::task_board::TaskBoardStore;
use crate::tools::parse_tool_call_arguments;
use crate::tools::web_search::{
    dispatch_to_tool_json_async, WebSearchDispatchContext, WebSearchInvokeContext,
    WebSearchTokenSink,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::PlannedMethod;

pub struct PlannerToolPassInput<'a> {
    pub store: Arc<TaskBoardStore>,
    pub store_key: &'a str,
    pub conversation_id: &'a str,
    pub settings: &'a ModelSettings,
    pub cancel: &'a CancellationToken,
    pub llm_stats: &'a mut ConversationLlmStats,
    pub run_id: &'a str,
    pub work_items_enabled: bool,
    pub history: &'a [ChatMessage],
}

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
        "web_search" => dispatch_web_search(input, &args, tc).await,
        "session_search" => dispatch_session_search(input, &args),
        "task_board_init" => dispatch_task_board(input, "init", &args).await,
        "task_board_replace" => dispatch_task_board(input, "replace", &args).await,
        other => Err(anyhow!("planner: unknown tool {other}")),
    }
}

async fn dispatch_web_search(
    input: &mut PlannerToolPassInput<'_>,
    args: &Value,
    tc: &ToolCall,
) -> Result<PlannerToolOutcome> {
    let scope = AgentInstanceScope::new(input.run_id, input.conversation_id, "task_board_planner");
    let (stream, _rx) = tokio::sync::mpsc::unbounded_channel();
    let ctx = WebSearchDispatchContext {
        settings: input.settings,
        agent_id: Some("computer"),
        args: args.clone(),
        cancel: input.cancel.clone(),
        stream,
        message_id: "planner_internal".into(),
        tool_call_id: tc.id.clone(),
        history: input.history,
        exclude_message_id: "planner_internal",
        invoke: WebSearchInvokeContext::Tool,
        token_sink: WebSearchTokenSink::Sub {
            stats: input.llm_stats,
            scope: &scope,
        },
        trace_id: None,
        scoped_message_id: None,
    };
    let (body, ok, err) = dispatch_to_tool_json_async(ctx).await?;
    if !ok {
        return Err(anyhow!(err.unwrap_or_else(|| "web_search failed".into())));
    }
    Ok(PlannerToolOutcome {
        tool_result: body,
        planned: None,
        board_len: 0,
    })
}

fn dispatch_session_search(
    input: &PlannerToolPassInput<'_>,
    args: &Value,
) -> Result<PlannerToolOutcome> {
    let mut bound = args.clone();
    if bound.get("conversation_id").and_then(|v| v.as_str()).unwrap_or("").trim().is_empty() {
        bound["conversation_id"] = json!(input.conversation_id);
    }
    let store = crate::conversation_store::global_store()
        .map_err(|e| anyhow!("session_search: conversation store unavailable: {e}"))?;
    let body = store.dispatch_search_tool(&bound)?;
    Ok(PlannerToolOutcome {
        tool_result: body,
        planned: None,
        board_len: 0,
    })
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
            board_len: doc.board.len(),
        });
    }
    let mut bound = args.clone();
    bound["_conversation_id"] = json!(input.store_key);
    bound["_task_board_work_items_enabled"] = json!(input.work_items_enabled);
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
