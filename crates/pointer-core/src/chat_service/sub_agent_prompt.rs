//! Sub-agent session bootstrap and per-round system prompt assembly.

use crate::agents::{
    computer_agent_body_for_tier, computer_communication_for_tier, delegatable_sub_agents_system_block,
    expand_agent_prompt_placeholders, normalize_allow_agents, rendered_communication_public_inject,
    AgentDef, AgentProfile, AgentTask, SessionInjectVars, DEFAULT_AGENT_ID,
};
use crate::extensions::{BeforeMainLlmCallContext, MessageLoopPromptsAfterContext};
use crate::models::{ChatMessage, Role, SystemPromptSections};
use crate::provider::OpenAIProvider;
use anyhow::{anyhow, Result};
use std::time::Instant;

use super::agent_tool_allowlist::resolve_agent_tools;
use super::app_state::AppState;
use super::prompts::push_env_to_cacheable;
use crate::task_board::sub_agent_hint::sub_agent_task_board_init_hint;
use crate::task_board::sub_agent_task_board_store_key;
use super::util::{new_id, now_ms};
use super::StreamTx;

pub(super) struct SubAgentSession {
    pub def: AgentDef,
    pub prompts: Vec<String>,
    pub tools_system_appendix: String,
    pub allowed_tools: Vec<String>,
    pub allow_agents: Vec<String>,
    pub sub_task_board_key: String,
    pub tool_approval_mode: String,
    pub local_history: Vec<ChatMessage>,
}

pub(super) struct SubAgentRoundPrompts {
    pub history_for_api: Vec<ChatMessage>,
    pub system_prompts: SystemPromptSections,
}

pub(super) fn init_sub_agent_session(
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    task: &AgentTask,
    enabled_skill_ids: &[String],
) -> Result<SubAgentSession> {
    let agent = state
        .agents
        .get(&task.agent_id)
        .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
        .ok_or_else(|| anyhow!("未找到 Agent: {}", task.agent_id))?;
    let def = agent.def().clone();
    let _ = enabled_skill_ids;
    let (skill_prompts, session_tools) = state.skills.progressive_context(&[]);
    let allowed_tools = resolve_agent_tools(&def, &session_tools, &state.tools);
    let allow_agents = normalize_allow_agents(&def.allow_agents);
    let sub_task_board_key =
        sub_agent_task_board_store_key(parent_task_board_store_key, task.id.trim());
    let session_vars = SessionInjectVars {
        workspace_root: provider.settings.workspace_root.trim(),
    };
    let expanded_role = expand_agent_prompt_placeholders(&agent.system_prompt(), &session_vars);
    let sub_agent_header = format!(
        "Sub-agent: {} ({})\nprofile: {:?}\ndescription: {}\n\n{}\n\nComplete only the subtask delivered in the next user message from the Supervisor. That message is task instructions (it may include a digest of prior task outputs) and does **not** include the main chat history. Finish by writing your full handoff directly in assistant Markdown content (conclusions, evidence, traces, open questions). When no further tool calls are required, the run ends and the lead reads the final assistant content from **`run_subagent`** result field **`content`**.\nAllowed tools: {}",
        def.name,
        def.id,
        def.profile,
        def.description,
        expanded_role,
        if allowed_tools.is_empty() {
            "none".into()
        } else {
            allowed_tools.join(", ")
        }
    );
    let mut prompts = Vec::new();
    if let Some(block) = rendered_communication_public_inject() {
        prompts.push(block);
    }
    prompts.push(sub_agent_header);
    if allowed_tools.iter().any(|t| t == "run_subagent") {
        if let Some(block) = delegatable_sub_agents_system_block(&state.agents, &allow_agents) {
            prompts.push(block);
        }
    }
    prompts.extend(skill_prompts);
    if let Some(hint) =
        sub_agent_task_board_init_hint(&state.task_board_store, &sub_task_board_key, &allowed_tools)
    {
        crate::task_board::observability::log_sub_agent_init_hint(
            conversation_id,
            task.id.trim(),
            &def.id,
        );
        prompts.push(hint);
    }

    let tools_system_appendix =
        crate::tools_system_appendix::generate_tools_system_appendix(
            &state.tools,
            &allowed_tools,
        );
    let tool_approval_mode = state.effective_settings().tool_approval_mode;
    let local_history = vec![ChatMessage {
        id: new_id("sub_task"),
        role: Role::User,
        content: task.instruction.clone(),
        status: "done".into(),
        created_at: now_ms(),
        tool_calls: None,
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
            }];

    Ok(SubAgentSession {
        def,
        prompts,
        tools_system_appendix,
        allowed_tools,
        allow_agents,
        sub_task_board_key,
        tool_approval_mode,
        local_history,
    })
}

pub(super) async fn prepare_sub_agent_round_prompts(
    state: &AppState,
    stream: &StreamTx,
    conversation_id: &str,
    message_id: &str,
    task_id: &str,
    round_message_id: &str,
    local_history: &[ChatMessage],
    base_prompts: &[String],
    tools_system_appendix: &str,
    sub_task_board_key: &str,
    def: &AgentDef,
    user_dynamic_inject_enabled: bool,
) -> Result<SubAgentRoundPrompts> {
    let round_prep = Instant::now();
    let t = Instant::now();
    let mut history_for_api = local_history.to_vec();
    let clone_ms = t.elapsed().as_millis();
    let mut prompts_after_ctx = MessageLoopPromptsAfterContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: def.profile.clone(),
        messages: &mut history_for_api,
        conversation_id,
        stream: Some(stream),
        round_assistant_message_id: Some(message_id.to_string()),
        round_screen_dump_prefix: Some(round_message_id.to_string()),
        task_board_store: state.task_board_store.clone(),
        task_board_store_key: sub_task_board_key,
        user_dynamic_inject_enabled,
    };
    let t = Instant::now();
    state
        .extensions
        .run_message_loop_prompts_after(&mut prompts_after_ctx)
        .await?;
    let message_loop_prompts_after_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut cacheable = base_prompts.to_vec();
    if def.profile == AgentProfile::Computer {
        let tier = state.computer_state.tier_for_conversation(conversation_id);
        let tier_slice = format!(
            "{}\n\n---\n\n{}",
            computer_communication_for_tier(tier),
            computer_agent_body_for_tier(tier)
        );
        if cacheable.len() > 1 {
            cacheable.insert(1, tier_slice);
        } else {
            cacheable.push(tier_slice);
        }
    }
    if !tools_system_appendix.is_empty() {
        cacheable.push(tools_system_appendix.to_string());
    }
    push_env_to_cacheable(&mut cacheable);
    let assemble_system_prompts_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut dynamic = Vec::new();
    let mut before_llm_ctx = BeforeMainLlmCallContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: def.profile.clone(),
        system_prompts_dynamic: &mut dynamic,
        conversation_id,
        task_board_store: state.task_board_store.clone(),
        task_board_store_key: sub_task_board_key,
    };
    state
        .extensions
        .run_before_main_llm_call(&mut before_llm_ctx)
        .await?;
    if !user_dynamic_inject_enabled {
        crate::extensions::task_board_hook::append_task_board_dynamic_block(
            &mut dynamic,
            state.task_board_store.as_ref(),
            sub_task_board_key,
            conversation_id,
            &def.profile,
        );
    }
    if let Some(parent_block) = state
        .task_board_store
        .parent_tunnel_for_child(sub_task_board_key, task_id)
    {
        dynamic.push(parent_block);
    }
    let before_main_llm_tail_ms = t.elapsed().as_millis();
    log::info!(
        "run_chat supervisor_sub_agent pre_stream_chat conversation_id={} task_id={} message_id={} local_history_messages={} clone_ms={} message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
        conversation_id,
        task_id,
        message_id,
        local_history.len(),
        clone_ms,
        message_loop_prompts_after_ms,
        assemble_system_prompts_ms,
        before_main_llm_tail_ms,
        round_prep.elapsed().as_millis(),
    );

    Ok(SubAgentRoundPrompts {
        history_for_api,
        system_prompts: SystemPromptSections { cacheable, dynamic },
    })
}
