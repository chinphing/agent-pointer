//! Sub-agent session bootstrap and per-round system prompt assembly.

use crate::agents::{
    expand_agent_prompt_placeholders, rendered_communication_public_inject, AgentDef, AgentTask,
    SessionInjectVars, DEFAULT_AGENT_ID,
};
use crate::extensions::{BeforeMainLlmCallContext, MessageLoopPromptsAfterContext};
use crate::models::{ChatMessage, Role, SystemPromptSections};
use crate::provider::OpenAIProvider;
use crate::storage;
use anyhow::{anyhow, Result};
use std::time::Instant;

use super::agent_tool_allowlist::resolve_agent_tools;
use super::app_state::AppState;
use super::prompts::push_env_and_json_wire_tail_to_cacheable;
use crate::task_board::sub_agent_task_board_store_key;
use super::util::{new_id, now_ms};
use super::StreamTx;

pub(super) struct SubAgentSession {
    pub def: AgentDef,
    pub prompts: Vec<String>,
    pub tools_system_appendix: String,
    pub allowed_tools: Vec<String>,
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
    task: &AgentTask,
    enabled_skill_ids: &[String],
) -> Result<SubAgentSession> {
    let agent = state
        .agents
        .get(&task.agent_id)
        .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
        .ok_or_else(|| anyhow!("未找到 Agent: {}", task.agent_id))?;
    let def = agent.def().clone();
    let mut skill_ids = enabled_skill_ids.to_vec();
    if !def.access_policy.allow_skills.is_empty() {
        skill_ids.retain(|id| def.access_policy.allow_skills.contains(id));
    }
    skill_ids.retain(|id| !def.access_policy.deny_skills.contains(id));
    skill_ids.sort();
    skill_ids.dedup();

    let (skill_prompts, session_tools) = state.skills.progressive_context(&skill_ids);
    let mut allowed_tools = resolve_agent_tools(&def, &session_tools, &state.tools);
    allowed_tools.retain(|t| t != "run_subagent");
    let sub_task_board_key = sub_agent_task_board_store_key(conversation_id, task.id.trim());
    let session_vars = SessionInjectVars {
        workspace_root: provider.settings.workspace_root.trim(),
    };
    let expanded_role = expand_agent_prompt_placeholders(&agent.system_prompt(), &session_vars);
    let sub_agent_header = format!(
        "Sub-agent: {} ({})\nprofile: {:?}\ndescription: {}\n\n{}\n\nComplete only the subtask delivered in the next user message from the Supervisor. That message is task instructions (it may include a digest of prior task outputs) and does **not** include the main chat history. Finish with the **`response`** tool: put your full handoff in **`tool_args.text`** as **Markdown** (conclusions, evidence, traces, open questions). The lead reads that Markdown from the **`run_subagent`** tool result field **`content`**.\nAllowed tools: {}",
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
    prompts.extend(skill_prompts);

    let tools_system_appendix =
        crate::tools_system_appendix::generate_tools_system_appendix(&state.tools, &allowed_tools);
    let tool_approval_mode = storage::load_settings()
        .map(|settings| settings.tool_approval_mode)
        .unwrap_or_else(|_| "auto".into());
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
        agent_id: None,
        agent_name: None,
        agent_trace: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
    }];

    Ok(SubAgentSession {
        def,
        prompts,
        tools_system_appendix,
        allowed_tools,
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
    };
    let t = Instant::now();
    state
        .extensions
        .run_message_loop_prompts_after(&mut prompts_after_ctx)
        .await?;
    let message_loop_prompts_after_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut cacheable = base_prompts.to_vec();
    if !tools_system_appendix.is_empty() {
        cacheable.push(tools_system_appendix.to_string());
    }
    push_env_and_json_wire_tail_to_cacheable(
        &mut cacheable,
        !tools_system_appendix.is_empty(),
    );
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
