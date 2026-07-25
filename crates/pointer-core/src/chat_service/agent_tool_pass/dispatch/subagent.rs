//! Nested `run_subagent` delegation (lead or sub scope).

use super::super::types::{
    ActiveAgentExecutionState, LeadToolPassConfig, SubToolPassConfig, ToolExecResult,
    ToolInvocationStats,
};

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

pub(in crate::chat_service::agent_tool_pass) struct PreparedOwnedSubagentInvocation {
    pub run_id: String,
    pub task: crate::agents::AgentTask,
    pub source: crate::chat_service::run_subagent_delegation::OwnedSubagentSource,
    /// Self-fork: UI nesting only. Explore: real spawn-depth consumption.
    pub child_spawn_depth: u32,
    pub max_spawn_depth: u32,
}

fn prepare_owned_parallel_task(
    args_value: &serde_json::Value,
    tool_call_id: &str,
) -> Result<(crate::agents::AgentTask, Option<String>), String> {
    let parsed = crate::tools::run_subagent::parse_run_subagent_args(args_value)?;
    if !parsed.is_parallel_wave_target() {
        return Err(format!(
            "parallel subagent wave does not support agentId={}",
            parsed.agent_id
        ));
    }
    crate::tools::run_subagent::validate_run_subagent_workspace(&parsed)?;
    let task_id = if parsed.task_id.trim().is_empty() {
        tool_call_id.to_string()
    } else {
        parsed.task_id.trim().to_string()
    };
    let title = if parsed.title.trim().is_empty() {
        format!("Delegated: {}", parsed.agent_id)
    } else {
        parsed.title
    };
    Ok((
        crate::agents::AgentTask {
            id: task_id,
            agent_id: parsed.agent_id,
            title,
            goal: parsed.goal,
            context: parsed.context.trim().to_string(),
            depends_on: vec![],
        },
        parsed.workspace_root,
    ))
}

/// Prepare `self` or `explore` for the owned-outcome parallel wave.
pub(in crate::chat_service::agent_tool_pass) fn prepare_owned_subagent_invocation(
    state: &super::super::super::app_state::AppState,
    active: &ActiveAgentExecutionState<'_>,
    run_id: &str,
    allow_agents: &[String],
    parent_spawn_depth: u32,
    max_spawn_depth: u32,
    workspace_root: &str,
    args_value: &serde_json::Value,
    tool_call_id: &str,
) -> Result<PreparedOwnedSubagentInvocation, String> {
    let (task, explicit_workspace_root) = prepare_owned_parallel_task(args_value, tool_call_id)?;
    let max_spawn_depth = max_spawn_depth.max(1);
    if task.agent_id == "self" {
        let workspace_root = match explicit_workspace_root.as_deref() {
            Some(root) => crate::workspace_delegation::resolve_explicit_workspace_root(root)
                .map_err(|err| format!("invalid workspaceRoot for self fork: {err}"))?,
            None => workspace_root.to_string(),
        };
        let trace_depth = parent_spawn_depth.saturating_add(1);
        return Ok(PreparedOwnedSubagentInvocation {
            run_id: run_id.to_string(),
            task,
            source: crate::chat_service::run_subagent_delegation::OwnedSubagentSource::SelfFork(
                build_active_self_fork_snapshot(state, active, &workspace_root),
            ),
            child_spawn_depth: trace_depth,
            max_spawn_depth: max_spawn_depth.max(trace_depth),
        });
    }

    let child_spawn_depth =
        crate::tools::run_subagent::validate_spawn_depth(parent_spawn_depth, max_spawn_depth)?;
    let target = crate::tools::run_subagent::validate_run_subagent_target(
        &state.agents,
        allow_agents,
        &active.def.id,
        &task.agent_id,
    )?;
    let crate::tools::run_subagent::RunSubagentTarget::Registered(def) = target else {
        return Err("expected registered explore target for parallel wave".into());
    };
    if def.id != "explore" {
        return Err(format!(
            "parallel subagent wave only supports self and explore; got {}",
            def.id
        ));
    }
    Ok(PreparedOwnedSubagentInvocation {
        run_id: run_id.to_string(),
        task,
        source: crate::chat_service::run_subagent_delegation::OwnedSubagentSource::Registered(def),
        child_spawn_depth,
        max_spawn_depth,
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
            current_agent_id: &lead_cfg.active.def.id,
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
            current_agent_id: &sub_cfg.active.def.id,
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
    use super::{build_active_self_fork_snapshot, prepare_owned_subagent_invocation};
    use crate::agents::{AccessPolicy, AgentDef, AgentProfile, AgentUiConfig, SkillsPolicy};
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

        let snapshot = build_active_self_fork_snapshot(&state, &active, "/captured/workspace");

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
            let prepared = prepare_owned_subagent_invocation(
                &state,
                &active,
                "coder-run",
                &[],
                1,
                2,
                "/coder/workspace",
                &serde_json::json!({"agentId": "self", "goal": goal}),
                call_id,
            )
            .expect("delegated coder self call should prepare");

            assert_eq!(prepared.run_id, "coder-run");
            assert_eq!(prepared.child_spawn_depth, 2);
            assert_eq!(prepared.task.id, call_id);
            assert!(matches!(
                prepared.source,
                crate::chat_service::run_subagent_delegation::OwnedSubagentSource::SelfFork(ref s)
                    if s.def.id == "coder" && s.skill_prompts == skill_prompts
            ));
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

        let prepared = prepare_owned_subagent_invocation(
            &state,
            &active,
            "coder-run",
            &[],
            2,
            2,
            "/coder/workspace",
            &serde_json::json!({"agentId": "self", "goal": "leaf work"}),
            "call-at-limit",
        )
        .expect("self fork must not consume cross-role spawn depth");

        assert_eq!(prepared.child_spawn_depth, 3);
        assert_eq!(prepared.task.agent_id, "self");
    }

    /// Delegating to your own id is rewritten to `self` during tool-pass preparation.
    /// An explicit workspace is preserved and becomes the fork's workspace.
    #[test]
    fn own_agent_id_rewritten_by_the_tool_pass_prepares_as_a_self_fork_in_explicit_workspace() {
        let state = crate::chat_service::AppState::new();
        let workspace = tempfile::tempdir().unwrap();
        let workspace_root = workspace.path().display().to_string();
        let canonical_workspace = workspace
            .path()
            .canonicalize()
            .unwrap()
            .display()
            .to_string();
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
        let mut args = serde_json::json!({
            "agentId": "coder",
            "goal": "What: split the work\nDone when: tests pass",
            "workspaceRoot": workspace_root
        });
        assert_eq!(
            crate::tools::run_subagent::apply_self_delegation_rewrite(
                &mut args,
                "coder",
                &["explore".to_string()],
            ),
            Ok(true)
        );

        let prepared = prepare_owned_subagent_invocation(
            &state,
            &active,
            "coder-run",
            &["explore".to_string()],
            1,
            2,
            "/parent/workspace",
            &args,
            "call-self-named",
        )
        .expect("rewritten self call belongs in the owned wave");

        assert_eq!(prepared.task.agent_id, "self");
        assert!(matches!(
            prepared.source,
            crate::chat_service::run_subagent_delegation::OwnedSubagentSource::SelfFork(ref s)
                if s.def.id == "coder" && s.workspace_root == canonical_workspace
        ));
    }

    #[test]
    fn prepare_owned_wave_accepts_explore_and_rejects_coder() {
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
        let allow = vec!["explore".to_string()];

        let explore = prepare_owned_subagent_invocation(
            &state,
            &active,
            "coder-run",
            &allow,
            0,
            2,
            "/coder/workspace",
            &serde_json::json!({
                "agentId": "explore",
                "goal": "Scenario: spec_map\nWhat: map auth\nDone when: report"
            }),
            "call-explore",
        )
        .expect("explore belongs in parallel wave");
        assert_eq!(explore.task.agent_id, "explore");
        assert_eq!(explore.child_spawn_depth, 1);
        assert!(matches!(
            explore.source,
            crate::chat_service::run_subagent_delegation::OwnedSubagentSource::Registered(ref d)
                if d.id == "explore"
        ));

        let coder_err = prepare_owned_subagent_invocation(
            &state,
            &active,
            "coder-run",
            &["coder".to_string()],
            0,
            2,
            "/coder/workspace",
            &serde_json::json!({
                "agentId": "coder",
                "goal": "What: patch\nDone when: tests pass",
                "workspaceRoot": "/tmp/project"
            }),
            "call-coder",
        );
        assert!(coder_err.is_err());
    }
}
