//! `input_at` async dispatch — type into a described control.
//!
//! Same shape as the mouse tools: `{target_desc, bbox, text}`. The pair is
//! grounded first so the click lands inside the field, then the existing
//! `InputTool` does the focus / clear / type / submit sequence unchanged.
//!
//! Lives on the async path for the same reason as `dispatch_mouse`: grounding
//! needs the agent's provider, which the sync `ToolHandler` closure does not
//! carry.

use crate::agents::computer::tools::tool_input::InputTool;
use crate::agents::computer::tools::InputBackend;
use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;
use super::grounding::resolve_point;

pub(super) async fn dispatch_input_at(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    tc: &ToolCall,
    args_value: Value,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let _ = tc;
    let mut args = args_value;
    let (x, y) = resolve_point(state, conversation_id, provider, &args, "", cancel).await?;
    {
        let obj = args
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("input_at 参数必须是对象"))?;
        obj.insert("x".to_string(), serde_json::json!(x));
        obj.insert("y".to_string(), serde_json::json!(y));
        obj.remove("target_desc");
        obj.remove("bbox");
    }

    let vision = state
        .computer_state
        .vision_state_for_conversation(conversation_id);
    let tool = InputTool::new(state.computer_state.control.clone(), vision);
    let out = tool.execute_with(InputBackend::At, &args)?;
    Ok((out, true, None))
}
