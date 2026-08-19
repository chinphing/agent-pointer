use crate::agents::AgentDef;
use crate::tools::ToolRegistry;

use super::agent_tool_allowlist::retain_inheritable_subagent_tools;

#[derive(Debug, Clone)]
pub struct SelfForkSnapshot {
    pub def: AgentDef,
    pub system_prompt: String,
    pub skill_ids: Vec<String>,
    pub skill_prompts: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub workspace_root: String,
}

pub fn build_self_fork_snapshot(
    current_def: &AgentDef,
    current_system_prompt: &str,
    current_skill_ids: &[String],
    current_skill_prompts: &[String],
    current_allowed_tools: &[String],
    workspace_root: &str,
    registry: &ToolRegistry,
) -> SelfForkSnapshot {
    let mut allowed_tools = current_allowed_tools.to_vec();
    retain_inheritable_subagent_tools(&mut allowed_tools, registry);

    SelfForkSnapshot {
        def: current_def.clone(),
        system_prompt: current_system_prompt.to_string(),
        skill_ids: current_skill_ids.to_vec(),
        skill_prompts: current_skill_prompts.to_vec(),
        allowed_tools,
        workspace_root: workspace_root.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_self_fork_snapshot, SelfForkSnapshot};
    use crate::agents::{AccessPolicy, AgentDef, AgentProfile, AgentUiConfig, SkillsPolicy};
    use crate::tools::{ToolEntry, ToolRegistry};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn current_def() -> AgentDef {
        AgentDef {
            id: "current-agent".into(),
            name: "Current Agent".into(),
            description: "Active parent definition".into(),
            role: "worker".into(),
            profile: AgentProfile::Coder,
            default_skill_ids: vec!["default-skill".into()],
            skills_policy: SkillsPolicy::InheritsFromParent,
            access_policy: AccessPolicy::default(),
            builtin: false,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec!["explore".into()],
            config: HashMap::new(),
            ui: AgentUiConfig::default(),
            plugin_id: None,
        }
    }

    fn register(registry: &ToolRegistry, name: &str, inheritable: bool) {
        registry.register(
            ToolEntry::new(
                name,
                "test:self_fork",
                "low",
                false,
                "test",
                Arc::new(|_| Ok(String::new())),
            )
            .with_subagent_inheritance(inheritable),
        );
    }

    #[test]
    fn snapshot_retains_active_parent_state_and_filters_inherited_tools() {
        let registry = ToolRegistry::new();
        register(&registry, "terminal", true);
        register(&registry, "run_subagent", false);
        register(&registry, "file_write", true);
        register(&registry, "lead_only", false);
        register(&registry, "file_edit", true);
        register(&registry, "task_board_abandon", false);

        let def = current_def();
        let skill_ids = vec!["skill-a".to_string(), "skill-b".to_string()];
        let skill_prompts = vec![
            "Resolved skill A prompt".to_string(),
            "Resolved skill B prompt".to_string(),
        ];
        let allowed_tools = vec![
            "terminal".to_string(),
            "missing".to_string(),
            "run_subagent".to_string(),
            "file_write".to_string(),
            "lead_only".to_string(),
            "file_edit".to_string(),
            "task_board_abandon".to_string(),
        ];

        let snapshot: SelfForkSnapshot = build_self_fork_snapshot(
            &def,
            "active system prompt",
            &skill_ids,
            &skill_prompts,
            &allowed_tools,
            "/workspace/current",
            &registry,
        );

        assert_eq!(snapshot.def.id, "current-agent");
        assert_eq!(snapshot.def.profile, AgentProfile::Coder);
        assert_eq!(snapshot.system_prompt, "active system prompt");
        assert_eq!(snapshot.skill_ids, skill_ids);
        assert_eq!(snapshot.skill_prompts, skill_prompts);
        assert_eq!(snapshot.workspace_root, "/workspace/current");
        assert_eq!(
            snapshot.allowed_tools,
            vec!["terminal", "file_write", "file_edit"]
        );
    }
}
