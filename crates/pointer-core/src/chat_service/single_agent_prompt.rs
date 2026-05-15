//! Per-round system prompt assembly for the single-agent loop (extensions + env tail).

use crate::agents::{expand_agent_prompt_placeholders, rendered_communication_public_inject, AgentPlan, AgentProfile, SessionInjectVars};
use crate::extensions::{BeforeMainLlmCallContext, MessageLoopPromptsAfterContext};
use crate::models::{ChatMessage, ModelSettings};
use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;

use super::app_state::AppState;
use super::prompts::{push_env_context_last_in_system_prompts, push_json_wire_format_tail};
use super::StreamTx;

pub(super) struct SingleAgentRoundPrompts {
    pub history_for_api: Vec<ChatMessage>,
    pub prompts_with_env: Vec<String>,
}

pub(super) async fn prepare_single_agent_round_prompts(
    state: &Arc<AppState>,
    stream: &StreamTx,
    conversation_id: &str,
    history: &[ChatMessage],
    agent_plan: &AgentPlan,
    settings: &ModelSettings,
    assistant_id: &str,
    lead_profile: AgentProfile,
    tools_system_appendix: String,
    tools_appendix_enabled: bool,
) -> Result<SingleAgentRoundPrompts> {
    let round_prep = Instant::now();
    let t = Instant::now();
    let mut history_for_api = history.to_vec();
    let clone_ms = t.elapsed().as_millis();
    let mut prompts_after_ctx = MessageLoopPromptsAfterContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: lead_profile.clone(),
        messages: &mut history_for_api,
        conversation_id,
        stream: Some(stream),
        round_assistant_message_id: Some(assistant_id.to_string()),
        round_screen_dump_prefix: None,
    };
    let t = Instant::now();
    state
        .extensions
        .run_message_loop_prompts_after(&mut prompts_after_ctx)
        .await?;
    let message_loop_prompts_after_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut prompts_with_env = Vec::new();
    let session_vars = SessionInjectVars {
        workspace_root: settings.workspace_root.trim(),
    };
    if let Some(block) = rendered_communication_public_inject() {
        prompts_with_env.push(block);
    }
    prompts_with_env.extend(
        agent_plan
            .system_prompts
            .iter()
            .map(|p| expand_agent_prompt_placeholders(p, &session_vars)),
    );
    if !tools_system_appendix.is_empty() {
        prompts_with_env.push(tools_system_appendix);
    }
    let assemble_system_prompts_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut before_llm_ctx = BeforeMainLlmCallContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: lead_profile,
        system_prompts: &mut prompts_with_env,
        conversation_id,
        task_board_store: state.task_board_store.clone(),
        task_board_store_key: conversation_id,
    };
    state
        .extensions
        .run_before_main_llm_call(&mut before_llm_ctx)
        .await?;
    push_env_context_last_in_system_prompts(&mut prompts_with_env);
    push_json_wire_format_tail(&mut prompts_with_env, tools_appendix_enabled);
    let before_main_llm_tail_ms = t.elapsed().as_millis();
    log::info!(
        "run_chat single_agent pre_stream_chat conversation_id={} assistant_id={} history_messages={} clone_ms={} message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
        conversation_id,
        assistant_id,
        history.len(),
        clone_ms,
        message_loop_prompts_after_ms,
        assemble_system_prompts_ms,
        before_main_llm_tail_ms,
        round_prep.elapsed().as_millis(),
    );

    Ok(SingleAgentRoundPrompts {
        history_for_api,
        prompts_with_env,
    })
}
