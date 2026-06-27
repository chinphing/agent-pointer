//! Sub-agent session bootstrap and per-round system prompt assembly.

use crate::agents::{
    delegatable_sub_agents_system_block, expand_agent_prompt_placeholders, normalize_allow_agents,
    AgentDef, AgentProfile, AgentTask, SessionInjectVars, DEFAULT_AGENT_ID,
};
use crate::extensions::{BeforeMainLlmCallContext, MessageLoopPromptsAfterContext};
use crate::models::{ChatMessage, Role, SystemPromptSections};
use crate::provider::OpenAIProvider;
use crate::tools::run_subagent::can_spawn_subagents;
use anyhow::{anyhow, Result};
use std::time::Instant;

use super::agent_tool_allowlist::resolve_agent_tools;
use super::app_state::AppState;
use super::prompts::{push_agent_role_cacheable_prompts, push_env_to_cacheable};
use super::sub_agent_task_prompt::{
    build_subagent_initial_user_message, build_subagent_spawn_depth_block,
    build_subagent_task_system_blocks, push_sub_agent_task_system_dynamic,
};
use crate::task_board::sub_agent_hint::sub_agent_task_board_init_hint;
use crate::task_board::sub_agent_task_board_store_key;
use super::emit::agent_trace_step_id;
use super::sub_message::{load_scoped_transcript, persist_sub_message, SubMessageLinkage};
use super::util::{new_id, now_ms};

pub(super) struct SubAgentSession {
    pub def: AgentDef,
    /// Sub-agent-only cacheable slices (handoff header, delegatable agents, skills, task-board hint).
    /// Role/tier prompts are assembled per round via [`push_agent_role_cacheable_prompts`].
    pub session_extras: Vec<String>,
    /// Assigned task + spawn depth blocks (merged into system dynamic each round).
    pub task_dynamic_blocks: Vec<String>,
    pub tools_system_appendix: String,
    pub allowed_tools: Vec<String>,
    pub allow_agents: Vec<String>,
    pub sub_task_board_key: String,
    pub tool_approval_mode: String,
    pub local_history: Vec<ChatMessage>,
    pub spawn_depth: u32,
}

pub(super) struct SubAgentRoundPrompts {
    pub history_for_api: Vec<ChatMessage>,
    pub system_prompts: SystemPromptSections,
}

pub(super) fn init_sub_agent_session(
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    anchor_message_id: &str,
    parent_task_board_store_key: &str,
    task: &AgentTask,
    enabled_skill_ids: &[String],
    spawn_depth: u32,
    max_spawn_depth: u32,
) -> Result<SubAgentSession> {
    let agent = state
        .agents
        .get(&task.agent_id)
        .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
        .ok_or_else(|| anyhow!("未找到 Agent: {}", task.agent_id))?;
    let def = agent.def().clone();
    let _ = enabled_skill_ids;
    let skill_ids: Vec<String> = if crate::agents::sub_agent_inherits_session_skills(&def.id) {
        enabled_skill_ids.to_vec()
    } else {
        Vec::new()
    };
    let (skill_prompts, session_tools) = state.skills.progressive_context(&skill_ids);
    let mut allowed_tools = resolve_agent_tools(&def, &session_tools, &state.tools);
    let allow_agents = normalize_allow_agents(&def.allow_agents);
    let max_spawn_depth = max_spawn_depth.max(1);
    let spawn_can_delegate =
        can_spawn_subagents(spawn_depth, max_spawn_depth) && !allow_agents.is_empty();
    if !spawn_can_delegate {
        allowed_tools.retain(|t| t != "run_subagent");
    }
    if def.profile == AgentProfile::Computer
        && provider.settings.computer_standalone_planner_enabled
    {
        allowed_tools.retain(|t| t != "task_board_init");
    }
    let sub_task_board_key =
        sub_agent_task_board_store_key(parent_task_board_store_key, task.id.trim());
    let session_vars = SessionInjectVars {
        workspace_root: provider.settings.workspace_root.trim(),
    };
    let allowed_tools_line = if allowed_tools.is_empty() {
        "none".into()
    } else {
        allowed_tools.join(", ")
    };
    let handoff_footer = "Finish by writing your full handoff directly in assistant Markdown content \
        (conclusions, evidence, traces, open questions). When no further tool calls are required, \
        the run ends and the parent reads the final assistant content from **`run_subagent`** result field **`content`**.";
    let sub_agent_header = if def.profile == AgentProfile::Computer {
        format!(
            "Sub-agent: {} ({})\nprofile: {:?}\ndescription: {}\n\n\
             Your assigned task is in the system prompt under **Assigned task** (not main chat history). \
             {}\nAllowed tools: {}",
            def.name,
            def.id,
            def.profile,
            def.description,
            handoff_footer,
            allowed_tools_line,
        )
    } else {
        let expanded_role = expand_agent_prompt_placeholders(&agent.system_prompt(), &session_vars);
        format!(
            "Sub-agent: {} ({})\nprofile: {:?}\ndescription: {}\n\n{}\n\n\
             Your assigned task is in the system prompt under **Assigned task** (not main chat history). \
             {}\nAllowed tools: {}",
            def.name,
            def.id,
            def.profile,
            def.description,
            expanded_role,
            handoff_footer,
            allowed_tools_line,
        )
    };
    let mut session_extras = vec![sub_agent_header];
    if spawn_can_delegate && allowed_tools.iter().any(|t| t == "run_subagent") {
        if let Some(block) = delegatable_sub_agents_system_block(&state.agents, &allow_agents) {
            session_extras.push(block);
        }
    }
    session_extras.extend(skill_prompts);
    let planner_handles_init = def.profile == AgentProfile::Computer
        && provider.settings.computer_standalone_planner_enabled;
    if !planner_handles_init {
        if let Some(hint) =
            sub_agent_task_board_init_hint(&state.task_board_store, &sub_task_board_key, &allowed_tools)
        {
            crate::task_board::observability::log_sub_agent_init_hint(
                conversation_id,
                task.id.trim(),
                &def.id,
            );
            session_extras.push(hint);
        }
    }

    let mut task_dynamic_blocks = build_subagent_task_system_blocks(
        &task.goal,
        &task.context,
        provider.settings.workspace_root.as_str(),
    );
    task_dynamic_blocks.push(build_subagent_spawn_depth_block(
        spawn_depth,
        max_spawn_depth,
        spawn_can_delegate && allowed_tools.iter().any(|t| t == "run_subagent"),
    ));

    let tools_system_appendix =
        crate::tools_system_appendix::generate_tools_system_appendix(
            &state.tools,
            &allowed_tools,
        );
    let tool_approval_mode = state.effective_settings().tool_approval_mode;
    let linkage = SubMessageLinkage {
        anchor_message_id: anchor_message_id.to_string(),
        trace_id: agent_trace_step_id(&task.id, &task.agent_id),
        task_id: task.id.clone(),
        spawn_depth,
    };
    let local_history = match load_scoped_transcript(conversation_id, &linkage) {
        Ok(rows) if !rows.is_empty() => {
            log::info!(
                "sub_agent: resumed scoped transcript conversation_id={} trace_id={} messages={}",
                conversation_id,
                linkage.trace_id,
                rows.len()
            );
            rows
        }
        _ => {
            let stub = ChatMessage {
                id: new_id("sub_task"),
                role: Role::User,
                content: build_subagent_initial_user_message(),
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
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
            };
            persist_sub_message(conversation_id, &linkage, &stub);
            vec![stub]
        }
    };

    Ok(SubAgentSession {
        def,
        session_extras,
        task_dynamic_blocks,
        tools_system_appendix,
        allowed_tools,
        allow_agents,
        sub_task_board_key,
        tool_approval_mode,
        local_history,
        spawn_depth,
    })
}

pub(super) async fn prepare_sub_agent_round_prompts(
    ctx: super::context::SubAgentPromptContext<'_>,
) -> Result<SubAgentRoundPrompts> {
    let state = ctx.session.state;
    let stream = ctx.session.stream;
    let conversation_id = ctx.session.conversation_id;
    let message_id = ctx.message_id;
    let task_id = ctx.task_id;
    let round_message_id = ctx.round_message_id;
    let local_history = ctx.local_history;
    let session_extras = ctx.session_extras;
    let task_dynamic_blocks = ctx.task_dynamic_blocks;
    let tools_system_appendix = ctx.tools_system_appendix;
    let sub_task_board_key = ctx.sub_task_board_key;
    let def = ctx.def;
    let workspace_root = ctx.workspace_root;
    let user_dynamic_inject_enabled = ctx.user_dynamic_inject_enabled;
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
        planner_outcome: ctx.planner_outcome.clone(),
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
        workspace_root: workspace_root.trim(),
    };
    push_agent_role_cacheable_prompts(
        &mut cacheable,
        &def.profile,
        state.computer_state.as_ref(),
        conversation_id,
        &session_vars,
        &[],
    );
    cacheable.extend(session_extras.iter().cloned());
    if !tools_system_appendix.is_empty() {
        cacheable.push(tools_system_appendix.to_string());
    }
    push_env_to_cacheable(&mut cacheable);
    let assemble_system_prompts_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let mut dynamic = Vec::new();
    push_sub_agent_task_system_dynamic(
        &mut dynamic,
        task_dynamic_blocks,
        state.task_board_store.as_ref(),
        sub_task_board_key,
        task_id,
    );
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
    let before_main_llm_tail_ms = t.elapsed().as_millis();
    log::info!(
        "run_chat supervisor_sub_agent pre_stream_chat conversation_id={} task_id={} message_id={} spawn_depth={} local_history_messages={} clone_ms={} message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
        conversation_id,
        task_id,
        message_id,
        ctx.spawn_depth,
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
