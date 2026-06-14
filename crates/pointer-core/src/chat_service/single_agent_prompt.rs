//! Per-round prompt assembly for the single-agent loop (extensions + system env).

use crate::agents::{AgentProfile, SessionInjectVars};
use crate::extensions::{BeforeMainLlmCallContext, MessageLoopPromptsAfterContext};
use crate::models::{ChatMessage, SystemPromptSections};
use anyhow::Result;
use std::time::Instant;

use super::prompts::{push_agent_role_cacheable_prompts, push_env_to_cacheable};

pub(super) struct SingleAgentRoundPrompts {
    pub history_for_api: Vec<ChatMessage>,
    pub system_prompts: SystemPromptSections,
}

pub(super) async fn prepare_single_agent_round_prompts(
    ctx: super::context::SingleAgentPromptContext<'_>,
) -> Result<SingleAgentRoundPrompts> {
    let state = &ctx.session.state;
    let stream = ctx.session.stream;
    let conversation_id = ctx.session.conversation_id;
    let history = ctx.history;
    let agent_plan = ctx.agent_plan;
    let settings = ctx.settings;
    let main_task_board_store_key = ctx.main_task_board_store_key;
    let assistant_id = ctx.assistant_id;
    let lead_profile = ctx.lead_profile.clone();
    let tools_system_appendix = ctx.tools_system_appendix;
    let _tools_appendix_enabled = ctx.tools_appendix_enabled;
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
        task_board_store: state.task_board_store.clone(),
        task_board_store_key: main_task_board_store_key,
        user_dynamic_inject_enabled: settings.user_dynamic_inject_enabled,
    };
    let t = Instant::now();
    state
        .extensions
        .run_message_loop_prompts_after(&mut prompts_after_ctx)
        .await?;
    let message_loop_prompts_after_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut cacheable = Vec::new();
    let session_vars = SessionInjectVars {
        workspace_root: settings.workspace_root.trim(),
    };
    let non_computer_prompts = if lead_profile == AgentProfile::Computer {
        &[][..]
    } else {
        agent_plan.system_prompts.as_slice()
    };
    push_agent_role_cacheable_prompts(
        &mut cacheable,
        &lead_profile,
        state.computer_state.as_ref(),
        conversation_id,
        &session_vars,
        non_computer_prompts,
    );
    if !tools_system_appendix.is_empty() {
        cacheable.push(tools_system_appendix);
    }
    push_env_to_cacheable(&mut cacheable);
    crate::memory::push_memory_to_cacheable(
        &mut cacheable,
        &state.memory_store,
        settings.memory_enabled,
        settings.user_profile_enabled,
    );
    let assemble_system_prompts_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut dynamic = Vec::new();
    let mut before_llm_ctx = BeforeMainLlmCallContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: lead_profile.clone(),
        system_prompts_dynamic: &mut dynamic,
        conversation_id,
        task_board_store: state.task_board_store.clone(),
        task_board_store_key: main_task_board_store_key,
    };
    state
        .extensions
        .run_before_main_llm_call(&mut before_llm_ctx)
        .await?;
    if !settings.user_dynamic_inject_enabled {
        crate::extensions::task_board_hook::append_task_board_dynamic_block(
            &mut dynamic,
            state.task_board_store.as_ref(),
            main_task_board_store_key,
            conversation_id,
            &lead_profile,
        );
    }
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
        system_prompts: SystemPromptSections { cacheable, dynamic },
    })
}
