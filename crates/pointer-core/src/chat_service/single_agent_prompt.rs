//! Per-round prompt assembly for the single-agent loop (extensions + system env).

use crate::agents::{AgentProfile, SessionInjectVars};
use crate::extensions::{BeforeMainLlmCallContext, MessageLoopPromptsAfterContext};
use crate::models::{ChatMessage, SystemPromptSections};
use anyhow::Result;
use std::time::Instant;

use super::prompts::{push_agent_role_cacheable_prompts, push_env_to_cacheable};

pub(super) struct SingleAgentRoundPrompts {
    /// Ephemeral API-only rows for this request (screen / task-board inject).
    pub injected_tail: Vec<ChatMessage>,
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
    let mut injected_tail = Vec::new();
    let mut prompts_after_ctx = MessageLoopPromptsAfterContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: lead_profile.clone(),
        base_messages: history,
        injected_tail: &mut injected_tail,
        conversation_id,
        stream: Some(stream),
        round_assistant_message_id: Some(assistant_id.to_string()),
        round_screen_dump_prefix: None,
        task_board_store: state.task_board_store.clone(),
        task_board_store_key: main_task_board_store_key,
        user_dynamic_inject_enabled: settings.user_dynamic_inject_enabled,
        workspace_root: settings.workspace_root.trim(),
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
    crate::i18n::push_ui_locale_reply_rule_to_cacheable(&mut cacheable, &settings.ui_locale);
    if !tools_system_appendix.is_empty() {
        cacheable.push(tools_system_appendix);
    }
    push_env_to_cacheable(&mut cacheable);
    crate::memory::push_memory_to_cacheable(
        &mut cacheable,
        &state.memory_store,
        crate::user_storage::session_user_id_for_conversation(conversation_id).as_str(),
        settings.memory_enabled,
        settings.user_profile_enabled,
    );
    crate::user_rules::push_user_coding_rules_to_cacheable(
        &mut cacheable,
        &settings.user_coding_rules,
    );
    crate::plugins::agents_md::push_agents_md_to_cacheable(
        &mut cacheable,
        settings.workspace_root.trim(),
        conversation_id,
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
    let injected_tail_len = injected_tail.len();
    let injected_image_slots: usize = injected_tail
        .iter()
        .map(|m| m.images_base64.as_ref().map(|v| v.len()).unwrap_or(0))
        .sum();
    log::info!(
        "run_chat single_agent pre_stream_chat conversation_id={} assistant_id={} history_messages={} injected_tail_messages={} injected_image_slots={} cloned_history=false message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
        conversation_id,
        assistant_id,
        history.len(),
        injected_tail_len,
        injected_image_slots,
        message_loop_prompts_after_ms,
        assemble_system_prompts_ms,
        before_main_llm_tail_ms,
        round_prep.elapsed().as_millis(),
    );

    Ok(SingleAgentRoundPrompts {
        injected_tail,
        system_prompts: SystemPromptSections { cacheable, dynamic },
    })
}
