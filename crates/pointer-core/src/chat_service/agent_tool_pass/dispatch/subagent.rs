//! Nested `run_subagent` delegation (lead or sub scope).

use super::super::types::{
    ActiveAgentExecutionState, LeadToolPassConfig, SubToolPassConfig, ToolExecResult,
    ToolInvocationStats,
};

pub(in crate::chat_service::agent_tool_pass) fn prepare_self_fork_task(
    args_value: &serde_json::Value,
    tool_call_id: &str,
) -> Result<crate::agents::AgentTask, String> {
    let parsed = crate::tools::run_subagent::parse_run_subagent_args(args_value)?;
    if !parsed.is_self_fork() {
        return Err("dedicated self-fork scheduler received a registered target".into());
    }
    crate::tools::run_subagent::validate_run_subagent_workspace(&parsed)?;
    let task_id = if parsed.task_id.trim().is_empty() {
        tool_call_id.to_string()
    } else {
        parsed.task_id.trim().to_string()
    };
    Ok(crate::agents::AgentTask {
        id: task_id,
        agent_id: parsed.agent_id,
        title: if parsed.title.trim().is_empty() {
            "Delegated: self".into()
        } else {
            parsed.title
        },
        goal: parsed.goal,
        context: parsed.context.trim().to_string(),
        depends_on: vec![],
    })
}

pub(in crate::chat_service::agent_tool_pass) fn build_active_self_fork_snapshot(
    state: &super::super::super::app_state::AppState,
    active: &ActiveAgentExecutionState<'_>,
    workspace_root: &str,
) -> crate::chat_service::self_fork::SelfForkSnapshot {
    crate::chat_service::self_fork::build_self_fork_snapshot(
        active.def,
        active.system_prompt,
        active.skill_ids,
        active.skill_prompts,
        active.allowed_tools,
        workspace_root,
        &state.tools,
    )
}

pub(in crate::chat_service::agent_tool_pass) struct PreparedSelfForkInvocation {
    pub run_id: String,
    pub task: crate::agents::AgentTask,
    pub snapshot: crate::chat_service::self_fork::SelfForkSnapshot,
    /// UI/trace nesting only; self forks do not consume cross-role spawn depth.
    pub trace_depth: u32,
    pub max_spawn_depth: u32,
}

pub(in crate::chat_service::agent_tool_pass) fn prepare_self_fork_invocation(
    state: &super::super::super::app_state::AppState,
    active: &ActiveAgentExecutionState<'_>,
    run_id: &str,
    parent_spawn_depth: u32,
    max_spawn_depth: u32,
    workspace_root: &str,
    args_value: &serde_json::Value,
    tool_call_id: &str,
) -> Result<PreparedSelfForkInvocation, String> {
    let task = prepare_self_fork_task(args_value, tool_call_id)?;
    let max_spawn_depth = max_spawn_depth.max(1);
    let trace_depth = parent_spawn_depth.saturating_add(1);
    Ok(PreparedSelfForkInvocation {
        run_id: run_id.to_string(),
        task,
        snapshot: build_active_self_fork_snapshot(state, active, workspace_root),
        trace_depth,
        max_spawn_depth: max_spawn_depth.max(trace_depth),
    })
}

pub(super) async fn dispatch_run_subagent(
    stream: &super::super::super::StreamTx,
    state: &super::super::super::app_state::AppState,
    provider: &crate::provider::OpenAIProvider,
    conversation_id: &str,
    parent_task_board_store_key: &str,
    message_id: &str,
    tc: &crate::models::ToolCall,
    args_value: serde_json::Value,
    cancel: &tokio_util::sync::CancellationToken,
    stats: &mut ToolInvocationStats<'_>,
    anchor_history: &mut Vec<crate::models::ChatMessage>,
    lead: Option<&mut LeadToolPassConfig<'_>>,
    sub: Option<&mut SubToolPassConfig<'_>>,
) -> ToolExecResult {
    let llm_stats = match stats {
        ToolInvocationStats::TokenSession(s) => &mut s.stats,
        ToolInvocationStats::Conversation(s) => s,
    };
    if let Some(lead_cfg) = lead {
        let mut deleg = super::super::super::context::SubagentDelegationContext {
            session: super::super::super::context::SessionRefs {
                stream,
                state,
                conversation_id,
                cancel,
            },
            parent_task_board_store_key,
            message_id,
            provider,
            run_id: lead_cfg.run_id,
            allow_agents: lead_cfg.allow_agents,
            enabled_skill_ids: lead_cfg.enabled_skill_ids.as_slice(),
            agent_skill_overrides: lead_cfg.agent_skill_overrides,
            agent_trace: lead_cfg.agent_trace,
            llm_stats,
            tool_call_id: &tc.id,
            args_value,
            parent_spawn_depth: 0,
            history: Some(anchor_history),
        };
        return super::super::super::run_subagent_delegation::run_subagent_delegation(&mut deleg)
            .await;
    }
    if let Some(sub_cfg) = sub {
        let empty_skills: &[String] = &[];
        let mut deleg = super::super::super::context::SubagentDelegationContext {
            session: super::super::super::context::SessionRefs {
                stream,
                state,
                conversation_id,
                cancel,
            },
            parent_task_board_store_key,
            message_id,
            provider,
            run_id: &sub_cfg.instance_scope.run_id,
            allow_agents: sub_cfg.allow_agents,
            enabled_skill_ids: empty_skills,
            agent_skill_overrides: sub_cfg.agent_skill_overrides,
            agent_trace: sub_cfg.agent_trace,
            llm_stats,
            tool_call_id: &tc.id,
            args_value,
            parent_spawn_depth: sub_cfg.spawn_depth,
            history: None,
        };
        return super::super::super::run_subagent_delegation::run_subagent_delegation(&mut deleg)
            .await;
    }
    unreachable!("run_subagent dispatch requires lead or sub scope")
}

#[cfg(test)]
mod self_fork_preparation_tests {
    use super::{build_active_self_fork_snapshot, prepare_self_fork_invocation};
    use crate::agents::{
        AccessPolicy, AgentDef, AgentProfile, AgentUiConfig, SkillsPolicy,
    };
    use crate::chat_service::agent_tool_pass::ActiveAgentExecutionState;
    use std::collections::HashMap;

    fn coder_def() -> AgentDef {
        AgentDef {
            id: "coder".into(),
            name: "Coder".into(),
            description: "active delegated coder".into(),
            role: "worker".into(),
            profile: AgentProfile::Coder,
            default_skill_ids: vec![],
            skills_policy: SkillsPolicy::DefaultsOnly,
            access_policy: AccessPolicy::default(),
            builtin: true,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec!["explore".into()],
            config: HashMap::new(),
            ui: AgentUiConfig::default(),
        }
    }

    #[test]
    fn snapshot_clones_captured_effective_state_without_recomputation() {
        let state = crate::chat_service::AppState::new();
        let def = coder_def();
        let skill_ids = vec!["captured-skill".to_string()];
        let skill_prompts = vec!["Captured resolved prompt".to_string()];
        let allowed_tools = vec!["terminal".to_string(), "run_subagent".to_string()];
        let mut mutable_enabled_ids = vec!["captured-skill".to_string()];
        let active = ActiveAgentExecutionState {
            def: &def,
            system_prompt: "captured system prompt",
            skill_ids: &skill_ids,
            skill_prompts: &skill_prompts,
            allowed_tools: &allowed_tools,
        };
        mutable_enabled_ids.clear();
        mutable_enabled_ids.push("changed-after-plan".into());

        let snapshot =
            build_active_self_fork_snapshot(&state, &active, "/captured/workspace");

        assert_eq!(snapshot.def.id, "coder");
        assert_eq!(snapshot.system_prompt, "captured system prompt");
        assert_eq!(snapshot.skill_ids, skill_ids);
        assert_eq!(mutable_enabled_ids, vec!["changed-after-plan"]);
        assert_eq!(snapshot.skill_prompts, skill_prompts);
        assert_eq!(snapshot.allowed_tools, vec!["terminal"]);
        assert_eq!(snapshot.workspace_root, "/captured/workspace");
    }

    #[test]
    fn delegated_coder_singleton_and_multiple_self_calls_prepare_from_sub_state() {
        let state = crate::chat_service::AppState::new();
        let def = coder_def();
        let skill_ids = vec!["coder-skill".to_string()];
        let skill_prompts = vec!["Coder resolved prompt".to_string()];
        let allowed_tools = vec!["terminal".to_string(), "run_subagent".to_string()];
        let active = ActiveAgentExecutionState {
            def: &def,
            system_prompt: "coder current prompt",
            skill_ids: &skill_ids,
            skill_prompts: &skill_prompts,
            allowed_tools: &allowed_tools,
        };

        for (call_id, goal) in [("call-1", "first"), ("call-2", "second")] {
            let prepared = prepare_self_fork_invocation(
                &state,
                &active,
                "coder-run",
                1,
                2,
                "/coder/workspace",
                &serde_json::json!({"agentId": "self", "goal": goal}),
                call_id,
            )
            .expect("delegated coder self call should prepare");

            assert_eq!(prepared.run_id, "coder-run");
            assert_eq!(prepared.trace_depth, 2);
            assert_eq!(prepared.task.id, call_id);
            assert_eq!(prepared.snapshot.def.id, "coder");
            assert_eq!(prepared.snapshot.skill_prompts, skill_prompts);
        }
    }

    #[test]
    fn delegated_coder_self_fork_prepares_at_cross_role_depth_limit() {
        let state = crate::chat_service::AppState::new();
        let def = coder_def();
        let skill_ids = vec![];
        let skill_prompts = vec![];
        let allowed_tools = vec!["run_subagent".to_string()];
        let active = ActiveAgentExecutionState {
            def: &def,
            system_prompt: "coder current prompt",
            skill_ids: &skill_ids,
            skill_prompts: &skill_prompts,
            allowed_tools: &allowed_tools,
        };

        let prepared = prepare_self_fork_invocation(
            &state,
            &active,
            "coder-run",
            2,
            2,
            "/coder/workspace",
            &serde_json::json!({"agentId": "self", "goal": "leaf work"}),
            "call-at-limit",
        )
        .expect("self fork must not consume cross-role spawn depth");

        assert_eq!(prepared.trace_depth, 3);
        assert_eq!(prepared.task.agent_id, "self");
    }
}
