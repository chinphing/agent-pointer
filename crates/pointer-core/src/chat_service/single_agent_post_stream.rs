//! Lead single-agent wrappers over shared `agent_post_stream`.

pub(super) use super::agent_post_stream::PostAssistantTurnAction;

use crate::agents::AgentPlan;
use crate::models::{AgentTrace, ChatMessage, ToolCall};

use super::app_state::AppState;

pub(super) fn build_assistant_message_after_stream(
    assistant_id: &str,
    raw_content_buf: &str,
    reasoning_buf: String,
    reasoning_in_messages: bool,
    final_tool_calls: &[ToolCall],
    xml_thoughts: Option<String>,
    agent_plan: &AgentPlan,
    agent_instance_id: Option<String>,
    agent_trace: &[AgentTrace],
    state: &AppState,
) -> ChatMessage {
    super::agent_post_stream::build_lead_assistant_message_after_stream(
        assistant_id,
        raw_content_buf,
        reasoning_buf,
        reasoning_in_messages,
        final_tool_calls,
        xml_thoughts,
        agent_plan,
        agent_instance_id,
        agent_trace,
        state,
    )
}

pub(super) fn build_final_reply_delivery_message(
    assistant_id: &str,
    tool_output: &str,
    agent_plan: &AgentPlan,
    agent_instance_id: Option<String>,
    state: &AppState,
) -> ChatMessage {
    super::agent_post_stream::build_final_reply_delivery_message(
        assistant_id,
        tool_output,
        agent_plan,
        agent_instance_id,
        state,
    )
}

pub(super) fn commit_assistant_turn(
    stream: &super::StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    assistant_id: &str,
    assistant_msg: &ChatMessage,
) {
    super::agent_post_stream::commit_lead_assistant_turn(
        stream,
        conversation_id,
        history,
        assistant_id,
        assistant_msg,
    );
}
