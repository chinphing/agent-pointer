//! Sub-agent session bootstrap and per-round system prompt assembly.

use crate::agent_instance_scope::AgentInstanceScope;
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

use super::agent_tool_allowlist::{resolve_agent_tools, retain_inheritable_subagent_tools};
use super::app_state::AppState;
use super::emit::agent_trace_step_id;
use super::prompts::{push_agent_role_cacheable_prompts, push_env_to_cacheable};
use super::self_fork::SelfForkSnapshot;
use super::sub_agent_task_prompt::{
    build_subagent_initial_user_message, build_subagent_spawn_depth_block,
    build_subagent_task_system_blocks, push_sub_agent_task_system_dynamic, SubAgentSpawnCapability,
};
use super::sub_message::{persist_sub_message, SubMessageLinkage};
use super::util::{new_id, now_ms};
use crate::task_board::{
    sub_agent_task_board_store_key, sub_agent_task_board_store_key_for_instance,
};

pub(super) enum SubAgentDefinitionSource<'a> {
    Registered(&'a AgentTask),
    Snapshot(&'a SelfForkSnapshot),
}

impl SubAgentDefinitionSource<'_> {
    pub(super) fn new_instance_scope(
        &self,
        run_id: &str,
        conversation_id: &str,
    ) -> AgentInstanceScope {
        let agent_role_id = match self {
            Self::Registered(task) => task.agent_id.as_str(),
            Self::Snapshot(snapshot) => snapshot.def.id.as_str(),
        };
        AgentInstanceScope::new(run_id, conversation_id, agent_role_id)
    }
}

pub(super) fn resolve_child_task_board_store_key(
    parent_task_board_store_key: &str,
    task: &AgentTask,
    agent_instance_id: &str,
) -> String {
    // Parallel-wave targets (`self`, `explore`) isolate boards per instance so concurrent
    // same-taskId children do not collide.
    if task.agent_id.trim() == "self" || task.agent_id.trim() == "explore" {
        sub_agent_task_board_store_key_for_instance(
            parent_task_board_store_key,
            task.id.trim(),
            agent_instance_id,
        )
    } else {
        sub_agent_task_board_store_key(parent_task_board_store_key, task.id.trim())
    }
}

fn resolve_subagent_spawn_capability(
    allowed_tools: &[String],
    allow_agents: &[String],
    spawn_depth: u32,
    max_spawn_depth: u32,
) -> SubAgentSpawnCapability {
    if !allowed_tools.iter().any(|tool| tool == "run_subagent") {
        return SubAgentSpawnCapability::None;
    }
    if can_spawn_subagents(spawn_depth, max_spawn_depth) && !allow_agents.is_empty() {
        SubAgentSpawnCapability::Registered
    } else {
        SubAgentSpawnCapability::SelfOnly
    }
}

pub(super) fn sub_agent_trace_id(
    task: &AgentTask,
    def: &AgentDef,
    instance_scope: &AgentInstanceScope,
) -> String {
    if task.agent_id.trim() == "self" || task.agent_id.trim() == "explore" {
        let scoped_agent_id = agent_trace_step_id(&instance_scope.agent_instance_id, &def.id);
        agent_trace_step_id(&task.id, &scoped_agent_id)
    } else {
        agent_trace_step_id(&task.id, &def.id)
    }
}

pub(super) struct SubAgentSession {
    pub def: AgentDef,
    pub system_prompt: String,
    pub skill_ids: Vec<String>,
    pub skill_prompts: Vec<String>,
    pub instance_scope: AgentInstanceScope,
    pub trace_id: String,
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
    /// Ephemeral API-only rows for this request (screen / task-board inject).
    pub injected_tail: Vec<ChatMessage>,
    pub system_prompts: SystemPromptSections,
}

fn fresh_sub_agent_local_history() -> Vec<ChatMessage> {
    vec![ChatMessage {
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
    }]
}

fn build_sub_agent_linkage(
    anchor_message_id: &str,
    task: &AgentTask,
    def: &AgentDef,
    instance_scope: &AgentInstanceScope,
    spawn_depth: u32,
) -> SubMessageLinkage {
    SubMessageLinkage {
        anchor_message_id: anchor_message_id.to_string(),
        trace_id: sub_agent_trace_id(task, def, instance_scope),
        task_id: task.id.clone(),
        spawn_depth,
        agent_instance_id: instance_scope.agent_instance_id.clone(),
    }
}

pub(super) fn init_sub_agent_session(
    state: &AppState,
    provider: &OpenAIProvider,
    conversation_id: &str,
    anchor_message_id: &str,
    parent_task_board_store_key: &str,
    task: &AgentTask,
    definition_source: &SubAgentDefinitionSource<'_>,
    instance_scope: &AgentInstanceScope,
    enabled_skill_ids: &[String],
    agent_skill_overrides: &std::collections::HashMap<String, Vec<String>>,
    spawn_depth: u32,
    max_spawn_depth: u32,
) -> Result<SubAgentSession> {
    let (def, system_prompt, skill_ids, skill_prompts, allowed_tools, allow_agents, workspace_root) =
        match definition_source {
            SubAgentDefinitionSource::Registered(source_task) => {
                let agent = state
                    .agents
                    .get(&source_task.agent_id)
                    .or_else(|| state.agents.get(DEFAULT_AGENT_ID))
                    .ok_or_else(|| anyhow!("未找到 Agent: {}", source_task.agent_id))?;
                let def = agent.def().clone();
                let skill_ids = crate::agents::sub_agent_skill_ids(
                    &def,
                    enabled_skill_ids,
                    agent_skill_overrides,
                );
                let (skill_prompts, session_tools) = state.skills.progressive_context(&skill_ids);
                let allowed_tools = resolve_agent_tools(&def, &session_tools, &state.tools);
                let allow_agents = normalize_allow_agents(&def.allow_agents);
                (
                    def,
                    agent.system_prompt(),
                    skill_ids,
                    skill_prompts,
                    allowed_tools,
                    allow_agents,
                    provider.settings.workspace_root.trim().to_string(),
                )
            }
            SubAgentDefinitionSource::Snapshot(snapshot) => {
                let mut allowed_tools = snapshot.allowed_tools.clone();
                retain_inheritable_subagent_tools(&mut allowed_tools, &state.tools);
                allowed_tools.retain(|name| name != "run_subagent");
                (
                    snapshot.def.clone(),
                    snapshot.system_prompt.clone(),
                    snapshot.skill_ids.clone(),
                    snapshot.skill_prompts.clone(),
                    allowed_tools,
                    Vec::new(),
                    snapshot.workspace_root.trim().to_string(),
                )
            }
        };
    let max_spawn_depth = max_spawn_depth.max(1);
    let spawn_capability = resolve_subagent_spawn_capability(
        &allowed_tools,
        &allow_agents,
        spawn_depth,
        max_spawn_depth,
    );
    let sub_task_board_key = resolve_child_task_board_store_key(
        parent_task_board_store_key,
        task,
        &instance_scope.agent_instance_id,
    );
    let session_vars = SessionInjectVars {
        workspace_root: workspace_root.as_str(),
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
        let expanded_role = expand_agent_prompt_placeholders(&system_prompt, &session_vars);
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
    if spawn_capability == SubAgentSpawnCapability::Registered {
        if let Some(block) = delegatable_sub_agents_system_block(&state.agents, &allow_agents) {
            session_extras.push(block);
        }
    }
    session_extras.extend(skill_prompts.clone());
    let mut task_dynamic_blocks =
        build_subagent_task_system_blocks(&task.goal, &task.context, workspace_root.as_str());
    task_dynamic_blocks.push(build_subagent_spawn_depth_block(
        spawn_depth,
        max_spawn_depth,
        spawn_capability,
    ));

    let tools_system_appendix =
        crate::tools_system_appendix::generate_tools_system_appendix(&state.tools, &allowed_tools);
    let tool_approval_mode = state.effective_settings().tool_approval_mode;
    let linkage =
        build_sub_agent_linkage(anchor_message_id, task, &def, &instance_scope, spawn_depth);
    let local_history = fresh_sub_agent_local_history();
    let stub = &local_history[0];
    persist_sub_message(conversation_id, &linkage, stub);
    log::info!(
        "sub_agent: initialized fresh local history conversation_id={} trace_id={} message_id={}",
        conversation_id,
        linkage.trace_id,
        stub.id
    );

    Ok(SubAgentSession {
        def,
        system_prompt,
        skill_ids,
        skill_prompts,
        instance_scope: instance_scope.clone(),
        trace_id: linkage.trace_id,
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
    let mut injected_tail = Vec::new();
    let mut prompts_after_ctx = MessageLoopPromptsAfterContext {
        computer_state: state.computer_state.as_ref(),
        lead_agent_profile: def.profile.clone(),
        base_messages: local_history,
        injected_tail: &mut injected_tail,
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
    let injected_tail_len = injected_tail.len();
    let injected_image_slots: usize = injected_tail
        .iter()
        .map(|m| m.images_base64.as_ref().map(|v| v.len()).unwrap_or(0))
        .sum();
    log::info!(
        "run_chat supervisor_sub_agent pre_stream_chat conversation_id={} task_id={} message_id={} spawn_depth={} local_history_messages={} injected_tail_messages={} injected_image_slots={} cloned_history=false message_loop_prompts_after_ms={} assemble_system_prompts_ms={} before_main_llm_tail_ms={} pre_stream_total_ms={}",
        conversation_id,
        task_id,
        message_id,
        ctx.spawn_depth,
        local_history.len(),
        injected_tail_len,
        injected_image_slots,
        message_loop_prompts_after_ms,
        assemble_system_prompts_ms,
        before_main_llm_tail_ms,
        round_prep.elapsed().as_millis(),
    );

    Ok(SubAgentRoundPrompts {
        injected_tail,
        system_prompts: SystemPromptSections { cacheable, dynamic },
    })
}

#[cfg(test)]
mod definition_source_tests {
    use super::{
        build_sub_agent_linkage, fresh_sub_agent_local_history, resolve_child_task_board_store_key,
        resolve_subagent_spawn_capability, SubAgentDefinitionSource,
    };
    use crate::agents::{
        AccessPolicy, AgentDef, AgentProfile, AgentTask, AgentUiConfig, SkillsPolicy,
    };
    use crate::chat_service::self_fork::SelfForkSnapshot;
    use std::collections::HashMap;

    fn snapshot() -> SelfForkSnapshot {
        SelfForkSnapshot {
            def: AgentDef {
                id: "active-parent".into(),
                name: "Active Parent".into(),
                description: "snapshot".into(),
                role: "worker".into(),
                profile: AgentProfile::Coder,
                default_skill_ids: vec![],
                skills_policy: SkillsPolicy::InheritsFromParent,
                access_policy: AccessPolicy::default(),
                builtin: false,
                enabled: true,
                tool_names: vec![],
                source: None,
                resource_files: vec![],
                allow_agents: vec![],
                config: HashMap::new(),
                ui: AgentUiConfig::default(),
            },
            system_prompt: "active prompt".into(),
            skill_ids: vec!["skill-a".into()],
            skill_prompts: vec!["Skill A prompt".into()],
            allowed_tools: vec!["terminal".into()],
            workspace_root: "/active/workspace".into(),
        }
    }

    #[test]
    fn snapshot_source_creates_unique_instance_scopes_for_active_parent_role() {
        let snapshot = snapshot();
        let source = SubAgentDefinitionSource::Snapshot(&snapshot);

        let first = source.new_instance_scope("run", "conversation");
        let second = source.new_instance_scope("run", "conversation");

        assert_eq!(first.agent_role_id, "active-parent");
        assert_eq!(second.agent_role_id, "active-parent");
        assert_ne!(first.agent_instance_id, second.agent_instance_id);
    }

    #[test]
    fn every_sub_agent_session_starts_with_fresh_local_history() {
        let first = fresh_sub_agent_local_history();
        let second = fresh_sub_agent_local_history();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert!(matches!(first[0].role, crate::models::Role::User));
        assert!(matches!(second[0].role, crate::models::Role::User));
        assert_ne!(first[0].id, second[0].id);
    }

    #[test]
    fn snapshot_linkage_uses_unique_instance_scope_id() {
        let snapshot = snapshot();
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "self".into(),
            title: "Fork".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };

        let source = SubAgentDefinitionSource::Snapshot(&snapshot);
        let instance_scope = source.new_instance_scope("run", "conversation");
        let linkage = build_sub_agent_linkage("anchor", &task, &snapshot.def, &instance_scope, 1);

        assert_eq!(
            linkage.trace_id,
            format!("task-1:{}:active-parent", instance_scope.agent_instance_id)
        );
        assert_eq!(linkage.agent_instance_id, instance_scope.agent_instance_id);
    }

    #[test]
    fn registered_linkage_keeps_legacy_task_and_agent_trace_id() {
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "coder".into(),
            title: "Coder".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let def = AgentDef {
            id: "coder".into(),
            name: "Coder".into(),
            description: "registered".into(),
            role: "worker".into(),
            profile: AgentProfile::Coder,
            default_skill_ids: vec![],
            skills_policy: SkillsPolicy::InheritsFromParent,
            access_policy: AccessPolicy::default(),
            builtin: false,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec![],
            config: HashMap::new(),
            ui: AgentUiConfig::default(),
        };
        let source = SubAgentDefinitionSource::Registered(&task);
        let instance_scope = source.new_instance_scope("run", "conversation");

        let linkage = build_sub_agent_linkage("anchor", &task, &def, &instance_scope, 1);

        assert_eq!(
            linkage.trace_id,
            crate::chat_service::emit::agent_trace_step_id("task-1", "coder")
        );
    }

    #[test]
    fn explore_linkage_trace_id_is_scoped_by_instance() {
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "explore".into(),
            title: "Explore".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let def = AgentDef {
            id: "explore".into(),
            name: "Explore".into(),
            description: "registered".into(),
            role: "worker".into(),
            profile: AgentProfile::Explore,
            default_skill_ids: vec![],
            skills_policy: SkillsPolicy::InheritsFromParent,
            access_policy: AccessPolicy::default(),
            builtin: false,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec![],
            config: HashMap::new(),
            ui: AgentUiConfig::default(),
        };
        let source = SubAgentDefinitionSource::Registered(&task);
        let instance_scope = source.new_instance_scope("run", "conversation");

        let linkage = build_sub_agent_linkage("anchor", &task, &def, &instance_scope, 1);

        // explore/self spawns are instance-scoped so repeated delegates of the
        // same task do not collide on one trace row.
        assert_eq!(
            linkage.trace_id,
            crate::chat_service::emit::agent_trace_step_id(
                "task-1",
                &crate::chat_service::emit::agent_trace_step_id(
                    &instance_scope.agent_instance_id,
                    "explore"
                )
            )
        );
    }

    #[test]
    fn registered_agent_at_depth_limit_keeps_self_only_run_subagent() {
        let mut allowed_tools = vec!["terminal".to_string(), "run_subagent".to_string()];
        let allow_agents = vec!["explore".to_string()];

        let capability = resolve_subagent_spawn_capability(&mut allowed_tools, &allow_agents, 2, 2);

        assert_eq!(
            capability,
            crate::chat_service::sub_agent_task_prompt::SubAgentSpawnCapability::SelfOnly
        );
        assert!(allowed_tools.iter().any(|tool| tool == "run_subagent"));
    }

    #[test]
    fn self_fork_child_board_keys_include_instance_and_do_not_collide() {
        let task = AgentTask {
            id: "shared-task".into(),
            agent_id: "self".into(),
            title: "Fork".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let first = resolve_child_task_board_store_key("parent", &task, "fork-a");
        let second = resolve_child_task_board_store_key("parent", &task, "fork-b");
        assert_ne!(first, second);
        assert!(first.contains("fork-a"));
        assert!(second.contains("fork-b"));
    }

    #[test]
    fn explore_child_board_keys_include_instance_like_self_fork() {
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "explore".into(),
            title: "Explore".into(),
            goal: "Inspect".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let key = resolve_child_task_board_store_key("parent", &task, "instance-a");
        assert!(key.contains("instance-a"));
        assert_ne!(
            key,
            crate::task_board::sub_agent_task_board_store_key("parent", "task-1")
        );
    }

    #[test]
    fn writer_registered_child_board_key_stays_task_scoped() {
        let task = AgentTask {
            id: "task-1".into(),
            agent_id: "coder".into(),
            title: "Coder".into(),
            goal: "Implement".into(),
            context: String::new(),
            depends_on: vec![],
        };
        let key = resolve_child_task_board_store_key("parent", &task, "instance-ignored");
        assert_eq!(
            key,
            crate::task_board::sub_agent_task_board_store_key("parent", "task-1")
        );
        assert!(!key.contains("instance-ignored"));
    }
}
