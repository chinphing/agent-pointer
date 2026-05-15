//! `run_subagent` tool: executed in the chat runtime (`run_chat_inner`), not via [`ToolRegistry::invoke`].
use crate::agents::{AgentDef, AgentRegistry};
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

const DOC: &str = include_str!("prompts/run_subagent.md");

pub fn register_all(reg: &ToolRegistry) {
    let handler: ToolHandler = Arc::new(|_args: Value| -> Result<String> {
        Err(anyhow!(
            "run_subagent is executed by the chat runtime, not synchronous invoke"
        ))
    });
    reg.register(ToolEntry::new(
        "run_subagent",
        "medium",
        false,
        DOC.trim(),
        handler,
    ));
}

/// Parse tool JSON for `run_subagent`. Returns `(agent_id, instruction, title, task_id)`.
pub fn parse_run_subagent_args(args: &Value) -> Result<(String, String, String, String), String> {
    let agent_id = args
        .get("agentId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing or empty agentId".to_string())?;
    let instruction = args
        .get("instruction")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing or empty instruction".to_string())?;
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
    Ok((
        agent_id.to_string(),
        instruction.to_string(),
        title,
        task_id,
    ))
}

/// `allow_agents` must be sorted and deduped (see [`crate::storage::load_settings`]).
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
            "allowAgents is empty; add worker ids under that setting before using run_subagent"
                .into(),
        );
    }
    if allow_agents.binary_search_by(|probe| probe.as_str().cmp(aid)).is_err() {
        return Err(format!(
            "agentId `{aid}` is not listed in allowAgents (settings)"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::register_builtin_agents;
    use crate::agents::AgentRegistry;
    use serde_json::json;

    #[test]
    fn parse_requires_agent_id_and_instruction() {
        assert!(parse_run_subagent_args(&json!({})).is_err());
        assert!(parse_run_subagent_args(&json!({"agentId": "coder"})).is_err());
        let ok = parse_run_subagent_args(&json!({
            "agentId": "coder",
            "instruction": "Do the thing"
        }));
        assert!(ok.is_ok());
        let (id, ins, title, tid) = ok.unwrap();
        assert_eq!(id, "coder");
        assert_eq!(ins, "Do the thing");
        assert!(title.is_empty());
        assert!(tid.is_empty());
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
    fn validate_rejects_supervisor_even_if_listed() {
        let reg = AgentRegistry::new();
        register_builtin_agents(&reg);
        let allow = vec!["supervisor".to_string()];
        let r = validate_run_subagent_target(&reg, &allow, "supervisor");
        assert!(r.is_err());
    }
}
