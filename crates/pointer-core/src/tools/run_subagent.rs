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
    reg.register(
        ToolEntry::new(
            "run_subagent",
            DOC_SOURCE,
            "medium",
            false,
            DOC.trim(),
            handler,
        )
        .with_subagent_inheritance(false),
    );
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

impl RunSubagentArgs {
    pub fn is_self_fork(&self) -> bool {
        self.agent_id == "self"
    }

    /// Targets that may share the owned-outcome parallel subagent wave.
    /// Registered writers / desktop agents stay serial.
    pub fn is_parallel_wave_target(&self) -> bool {
        self.is_self_fork() || self.agent_id == "explore"
    }
}

#[derive(Debug, Clone)]
pub enum RunSubagentTarget {
    SelfFork,
    Registered(AgentDef),
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
    current_agent_id: &str,
    agent_id: &str,
) -> Result<RunSubagentTarget, String> {
    let aid = agent_id.trim();
    if aid.is_empty() {
        return Err("agentId is empty".into());
    }
    if aid == "self" {
        return Ok(RunSubagentTarget::SelfFork);
    }
    if allow_agents.is_empty() {
        return Err(
            "allowAgents is empty on the current agent; add worker ids to its AGENT.md frontmatter before using run_subagent"
                .into(),
        );
    }
    if allow_agents
        .binary_search_by(|probe| probe.as_str().cmp(aid))
        .is_err()
    {
        let self_fork_hint = if aid == current_agent_id.trim() {
            " Use agentId `self` to fork the current agent."
        } else {
            ""
        };
        return Err(format!(
            "agentId `{aid}` is not listed in the current agent allowAgents (AGENT.md frontmatter).{self_fork_hint}"
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
    Ok(RunSubagentTarget::Registered(d))
}

/// Resolution for an `agentId` that names the calling agent itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfDelegation {
    /// Another worker, already `self`, or delegatable through the registered path.
    NotApplicable,
    /// Serve it as a self fork.
    Fork,
}

/// Decide how to serve an `agentId` that repeats the caller's own id.
///
/// Asking for your own id means "another instance of me", which is exactly a self
/// fork; the registered path can never serve it, since an agent's own id is normally
/// absent from `allowAgents` and lead ids are not workers. Ids that *are* listed keep
/// the registered path so an explicit self-listing still spawns a fresh worker.
pub fn resolve_self_delegation(
    current_agent_id: &str,
    allow_agents: &[String],
    parsed: &RunSubagentArgs,
) -> SelfDelegation {
    let aid = parsed.agent_id.trim();
    if aid.is_empty() || aid == "self" || aid != current_agent_id.trim() {
        return SelfDelegation::NotApplicable;
    }
    if allow_agents
        .binary_search_by(|probe| probe.as_str().cmp(aid))
        .is_ok()
    {
        return SelfDelegation::NotApplicable;
    }
    SelfDelegation::Fork
}

/// Rewrite a self-naming `agentId` to `self` in place; `Ok(true)` when rewritten.
///
/// Runs before batch planning so every later stage (wave planning, preparation,
/// execution, UI) sees one target id.
pub fn apply_self_delegation_rewrite(
    args: &mut Value,
    current_agent_id: &str,
    allow_agents: &[String],
) -> Result<bool, String> {
    // Malformed args are reported by the regular parse further down the pipeline.
    let Ok(parsed) = parse_run_subagent_args(args) else {
        return Ok(false);
    };
    match resolve_self_delegation(current_agent_id, allow_agents, &parsed) {
        SelfDelegation::NotApplicable => Ok(false),
        SelfDelegation::Fork => match args.as_object_mut() {
            Some(obj) => {
                obj.insert("agentId".to_string(), Value::String("self".to_string()));
                Ok(true)
            }
            None => Err("run_subagent arguments must be a JSON object".to_string()),
        },
    }
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
    fn self_target_is_a_self_fork() {
        let args = parse_run_subagent_args(&json!({
            "agentId": "self",
            "goal": "What: inspect backend\nDone when: report findings"
        }))
        .unwrap();
        assert!(args.is_self_fork());
        assert!(args.is_parallel_wave_target());
    }

    #[test]
    fn explore_is_parallel_wave_target_writers_are_not() {
        let explore = parse_run_subagent_args(&json!({
            "agentId": "explore",
            "goal": "Scenario: spec_map\nWhat: map auth\nDone when: report surfaces"
        }))
        .unwrap();
        assert!(explore.is_parallel_wave_target());
        assert!(!explore.is_self_fork());

        let coder = parse_run_subagent_args(&json!({
            "agentId": "coder",
            "goal": "What: patch\nDone when: tests pass",
            "workspaceRoot": "/tmp/project"
        }))
        .unwrap();
        assert!(!coder.is_parallel_wave_target());
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
    fn registered_target_remains_rejected_at_cross_role_depth_limit() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "explore",
            "goal": "inspect"
        }))
        .unwrap();

        assert!(!parsed.is_self_fork());
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
        let t = resolve_computer_operation_target("在 Chrome 中填写注册表单", "", "注册账号", None);
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
    fn validate_self_does_not_require_workspace_root() {
        let parsed = parse_run_subagent_args(&json!({
            "agentId": "self",
            "goal": "What: inspect backend\nDone when: report findings"
        }))
        .unwrap();
        assert!(validate_run_subagent_workspace(&parsed).is_ok());
    }

    #[test]
    fn self_target_does_not_require_allow_agents() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let resolved = validate_run_subagent_target(&reg, &[], "coder", "self").unwrap();
        assert!(matches!(resolved, RunSubagentTarget::SelfFork));
    }

    #[test]
    fn validate_rejects_missing_allow_list() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let r = validate_run_subagent_target(&reg, &[], "general", "coder");
        assert!(r.is_err());
    }

    #[test]
    fn validate_rejects_unknown_id() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["not_an_agent".to_string()];
        let r = validate_run_subagent_target(&reg, &allow, "general", "not_an_agent");
        assert!(r.is_err());
    }

    #[test]
    fn validate_rejects_disallowed_registered_id() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["explore".to_string()];
        let r = validate_run_subagent_target(&reg, &allow, "coder", "coder");
        assert_eq!(
            r.unwrap_err(),
            "agentId `coder` is not listed in the current agent allowAgents (AGENT.md frontmatter). Use agentId `self` to fork the current agent."
        );
    }

    #[test]
    fn validate_accepts_worker_in_allow_list() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["coder".to_string()];
        let RunSubagentTarget::Registered(d) =
            validate_run_subagent_target(&reg, &allow, "general", "coder").unwrap()
        else {
            panic!("expected registered target");
        };
        assert_eq!(d.id, "coder");
    }

    #[test]
    fn validate_rejects_removed_general_worker_even_when_listed() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec![
            "coder".to_string(),
            "computer".to_string(),
            "general-worker".to_string(),
        ];
        let r = validate_run_subagent_target(&reg, &allow, "general", "general-worker");
        assert!(r.is_err(), "removed general-worker must not resolve");
    }

    #[test]
    fn validate_accepts_explore_when_listed() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["explore".to_string()];
        let RunSubagentTarget::Registered(d) =
            validate_run_subagent_target(&reg, &allow, "coder", "explore").unwrap()
        else {
            panic!("expected registered target");
        };
        assert_eq!(d.id, "explore");
        assert_eq!(d.profile, AgentProfile::Explore);
    }

    #[test]
    fn own_agent_id_is_rewritten_to_a_self_fork() {
        let allow = vec!["explore".to_string()];
        let mut args = json!({
            "agentId": "coder",
            "goal": "What: split the work\nDone when: tests pass"
        });
        assert_eq!(
            apply_self_delegation_rewrite(&mut args, "coder", &allow),
            Ok(true)
        );
        assert_eq!(args["agentId"], json!("self"));
    }

    #[test]
    fn own_agent_id_with_workspace_is_rewritten_to_a_self_fork() {
        let allow: Vec<String> = vec![];
        let mut args = json!({
            "agentId": "coder",
            "goal": "What: split the work\nDone when: tests pass",
            "workspaceRoot": "/workspace/project/"
        });
        assert_eq!(
            apply_self_delegation_rewrite(&mut args, "coder", &allow),
            Ok(true)
        );
        assert_eq!(args["agentId"], json!("self"));
        assert_eq!(args["workspaceRoot"], json!("/workspace/project/"));
    }

    #[test]
    fn own_agent_id_with_another_workspace_is_rewritten() {
        let allow: Vec<String> = vec![];
        let mut args = json!({
            "agentId": "coder",
            "goal": "What: split the work\nDone when: tests pass",
            "workspaceRoot": "/other/project"
        });
        assert_eq!(
            apply_self_delegation_rewrite(&mut args, "coder", &allow),
            Ok(true)
        );
        assert_eq!(args["agentId"], json!("self"));
        assert_eq!(args["workspaceRoot"], json!("/other/project"));
    }

    #[test]
    fn listed_own_id_and_other_targets_keep_the_registered_path() {
        let allow = vec!["coder".to_string(), "explore".to_string()];
        for (current, requested) in [
            ("coder", "coder"),
            ("general", "explore"),
            ("coder", "self"),
        ] {
            let mut args = json!({
                "agentId": requested,
                "goal": "What: work\nDone when: done"
            });
            assert_eq!(
                apply_self_delegation_rewrite(&mut args, current, &allow),
                Ok(false),
                "{current} -> {requested}"
            );
            assert_eq!(args["agentId"], json!(requested));
        }
    }
}
