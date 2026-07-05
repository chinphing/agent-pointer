//! `run_subagent` tool: executed in the chat runtime (`run_chat_inner`), not via [`ToolRegistry::invoke`].
use crate::agents::{AgentDef, AgentRegistry};
use crate::models::ComputerOperationTarget;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const DOC: &str = include_str!("prompts/run_subagent.md");
const DOC_SOURCE: &str = "tools/prompts/run_subagent.md";

pub fn register_all(reg: &ToolRegistry) {
    let handler: ToolHandler = Arc::new(|_args: Value| -> Result<String> {
        Err(anyhow!(
            "run_subagent is executed by the chat runtime, not synchronous invoke"
        ))
    });
    reg.register(ToolEntry::new(
        "run_subagent",
        DOC_SOURCE,
        "medium",
        false,
        DOC.trim(),
        handler,
    ));
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSubagentArgs {
    pub agent_id: String,
    pub goal: String,
    pub context: String,
    pub title: String,
    pub task_id: String,
    pub workspace_root: Option<String>,
    pub computer_target: Option<ComputerOperationTarget>,
}

/// Child depth after delegating from `parent_spawn_depth` (lead = 0).
pub fn child_spawn_depth(parent_spawn_depth: u32) -> u32 {
    parent_spawn_depth.saturating_add(1)
}

/// Reject when the child would exceed `max_spawn_depth` (floor 1).
pub fn validate_spawn_depth(parent_spawn_depth: u32, max_spawn_depth: u32) -> Result<u32, String> {
    let max = max_spawn_depth.max(1);
    if parent_spawn_depth >= max {
        return Err(format!(
            "spawn depth limit: parent is at depth {parent_spawn_depth}, maxSubAgentSpawnDepth={max}"
        ));
    }
    let child = child_spawn_depth(parent_spawn_depth);
    if child > max {
        return Err(format!(
            "spawn depth limit: child would be at depth {child}, maxSubAgentSpawnDepth={max}"
        ));
    }
    Ok(child)
}

/// Whether an agent at `spawn_depth` may call `run_subagent` again.
pub fn can_spawn_subagents(spawn_depth: u32, max_spawn_depth: u32) -> bool {
    spawn_depth < max_spawn_depth.max(1)
}

/// Resolve whether a delegated computer task operates Pointer itself or external apps.
pub fn resolve_computer_operation_target(
    goal: &str,
    context: &str,
    title: &str,
    explicit: Option<ComputerOperationTarget>,
) -> ComputerOperationTarget {
    if let Some(target) = explicit {
        return target;
    }
    let blob = format!("{title}\n{goal}\n{context}").to_lowercase();
    const SELF_MARKERS: &[&str] = &[
        "pointer 设置",
        "pointer设置",
        "pointer 界面",
        "pointer界面",
        "pointer 客户端",
        "pointer客户端",
        "操作 pointer",
        "操作pointer",
        "在 pointer",
        "在pointer",
        "pointer app",
        "本应用",
        "当前应用",
        "本客户端",
        "本软件",
        "pointer 内",
        "pointer内",
    ];
    for marker in SELF_MARKERS {
        if blob.contains(marker) {
            return ComputerOperationTarget::SelfApp;
        }
    }
    ComputerOperationTarget::External
}

fn parse_computer_target(args: &Value) -> Option<ComputerOperationTarget> {
    let raw = args.get("computerTarget").and_then(|v| v.as_str())?.trim();
    match raw {
        "self" => Some(ComputerOperationTarget::SelfApp),
        "external" => Some(ComputerOperationTarget::External),
        _ => None,
    }
}

/// Parse tool JSON for `run_subagent`.
pub fn parse_run_subagent_args(args: &Value) -> Result<RunSubagentArgs, String> {
    let agent_id = args
        .get("agentId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing or empty agentId".to_string())?;
    let goal = args
        .get("goal")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing or empty goal".to_string())?;
    let context = args
        .get("context")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let task_id = args
        .get("taskId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let workspace_root = args
        .get("workspaceRoot")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let computer_target = parse_computer_target(args);
    Ok(RunSubagentArgs {
        agent_id: agent_id.to_string(),
        goal: goal.to_string(),
        context,
        title,
        task_id,
        workspace_root,
        computer_target,
    })
}

/// `allow_agents` must be sorted and deduped (see [`crate::agents::normalize_allow_agents`]).
pub fn validate_run_subagent_target(
    registry: &AgentRegistry,
    allow_agents: &[String],
    agent_id: &str,
) -> Result<AgentDef, String> {
    let aid = agent_id.trim();
    if aid.is_empty() {
        return Err("agentId is empty".into());
    }
    if allow_agents.is_empty() {
        return Err(
            "allowAgents is empty on the current agent; add worker ids to its AGENT.md frontmatter before using run_subagent"
                .into(),
        );
    }
    if allow_agents.binary_search_by(|probe| probe.as_str().cmp(aid)).is_err() {
        return Err(format!(
            "agentId `{aid}` is not listed in the current agent allowAgents (AGENT.md frontmatter)"
        ));
    }
    let exec = registry
        .get(aid)
        .ok_or_else(|| format!("unknown agent id `{aid}`"))?;
    let d = exec.def();
    if !d.enabled {
        return Err(format!("agent `{aid}` is disabled"));
    }
    if d.role != "worker" {
        return Err(format!(
            "agent `{aid}` is not a worker; run_subagent targets workers only"
        ));
    }
    Ok(d)
}

/// **`workspaceRoot`** is required when delegating to **`coder`**.
pub fn validate_run_subagent_workspace(parsed: &RunSubagentArgs) -> Result<(), String> {
    if parsed.agent_id.trim() != "coder" {
        return Ok(());
    }
    if parsed
        .workspace_root
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return Ok(());
    }
    Err(
        "missing or empty workspaceRoot: required when agentId is `coder` (skill root under ~/.pointer/skills/, user project path, or current conversation workspace)"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::register_builtin_agents;
    use crate::agents::AgentProfile;
    use crate::agents::AgentRegistry;
    use serde_json::json;

    #[test]
    fn parse_requires_agent_id_and_goal() {
        assert!(parse_run_subagent_args(&json!({})).is_err());
        assert!(parse_run_subagent_args(&json!({"agentId": "coder"})).is_err());
        assert!(parse_run_subagent_args(&json!({
            "agentId": "coder",
            "instruction": "legacy"
        }))
        .is_err());
        let ok = parse_run_subagent_args(&json!({
            "agentId": "coder",
            "goal": "Do the thing"
        }));
        assert!(ok.is_ok());
        let parsed = ok.unwrap();
        assert_eq!(parsed.agent_id, "coder");
        assert_eq!(parsed.goal, "Do the thing");
        assert!(parsed.context.is_empty());
        assert!(parsed.title.is_empty());
        assert!(parsed.task_id.is_empty());
        assert!(parsed.workspace_root.is_none());
    }

    #[test]
    fn parse_accepts_context() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "explore",
            "goal": "Scenario: spec_map\nMap auth.",
            "context": "Lead context:\n- grep done"
        }))
        .unwrap();
        assert!(parsed.context.contains("grep done"));
    }

    #[test]
    fn parse_accepts_computer_target() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "computer",
            "goal": "Open settings",
            "computerTarget": "self"
        }))
        .unwrap();
        assert_eq!(
            parsed.computer_target,
            Some(crate::models::ComputerOperationTarget::SelfApp)
        );
    }

    #[test]
    fn validate_spawn_depth_default_max_two() {
        assert_eq!(validate_spawn_depth(0, 2).unwrap(), 1);
        assert_eq!(validate_spawn_depth(1, 2).unwrap(), 2);
        assert!(validate_spawn_depth(2, 2).is_err());
    }

    #[test]
    fn can_spawn_subagents_at_depth_one_when_max_two() {
        assert!(can_spawn_subagents(1, 2));
        assert!(!can_spawn_subagents(2, 2));
    }

    #[test]
    fn resolve_computer_target_prefers_explicit() {
        let t = resolve_computer_operation_target(
            "操作 Chrome 浏览器",
            "",
            "外部任务",
            Some(crate::models::ComputerOperationTarget::SelfApp),
        );
        assert_eq!(t, crate::models::ComputerOperationTarget::SelfApp);
    }

    #[test]
    fn resolve_computer_target_infers_pointer_settings() {
        let t = resolve_computer_operation_target(
            "在 Pointer 设置页打开技能管理并启用 find-skills",
            "",
            "配置技能",
            None,
        );
        assert_eq!(t, crate::models::ComputerOperationTarget::SelfApp);
    }

    #[test]
    fn resolve_computer_target_defaults_external() {
        let t = resolve_computer_operation_target(
            "在 Chrome 中填写注册表单",
            "",
            "注册账号",
            None,
        );
        assert_eq!(t, crate::models::ComputerOperationTarget::External);
    }

    #[test]
    fn parse_accepts_workspace_root() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "coder",
            "goal": "Fix tests",
            "workspaceRoot": "/tmp/my-project"
        }))
        .unwrap();
        assert_eq!(parsed.workspace_root.as_deref(), Some("/tmp/my-project"));
    }

    #[test]
    fn validate_coder_requires_workspace_root() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "coder",
            "goal": "What: Fix tests.\nDone when: tests pass."
        }))
        .unwrap();
        assert!(validate_run_subagent_workspace(&parsed).is_err());

        let with_ws = parse_run_subagent_args(&json!({
            "agentId": "coder",
            "goal": "What: Fix tests.\nDone when: tests pass.",
            "workspaceRoot": "/tmp/my-project"
        }))
        .unwrap();
        assert!(validate_run_subagent_workspace(&with_ws).is_ok());
    }

    #[test]
    fn validate_explore_does_not_require_workspace_root() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "explore",
            "goal": "Scenario: spec_map\nWhat: Map auth.\nDone when: handoff complete."
        }))
        .unwrap();
        assert!(validate_run_subagent_workspace(&parsed).is_ok());
    }

    #[test]
    fn validate_rejects_missing_allow_list() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let r = validate_run_subagent_target(&reg, &[], "coder");
        assert!(r.is_err());
    }

    #[test]
    fn validate_rejects_unknown_id() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["coder".to_string()];
        let r = validate_run_subagent_target(&reg, &allow, "not_an_agent");
        assert!(r.is_err());
    }

    #[test]
    fn validate_accepts_worker_in_allow_list() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["coder".to_string()];
        let d = validate_run_subagent_target(&reg, &allow, "coder").unwrap();
        assert_eq!(d.id, "coder");
    }

    #[test]
    fn validate_accepts_general_worker_for_general_lead() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec![
            "coder".to_string(),
            "computer".to_string(),
            "general-worker".to_string(),
        ];
        let d = validate_run_subagent_target(&reg, &allow, "general-worker").unwrap();
        assert_eq!(d.id, "general-worker");
    }

    #[test]
    fn validate_accepts_explore_when_listed() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["explore".to_string()];
        let d = validate_run_subagent_target(&reg, &allow, "explore").unwrap();
        assert_eq!(d.id, "explore");
        assert_eq!(d.profile, AgentProfile::Explore);
    }

    #[test]
    fn validate_rejects_supervisor_even_if_listed() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["supervisor".to_string()];
        let r = validate_run_subagent_target(&reg, &allow, "supervisor");
        assert!(r.is_err());
    }
}
