//! Per-agent chat UI visibility defaults (merged with AGENT.md `ui` block).

use super::{AgentDef, AgentProfile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUiConfig {
    #[serde(default, rename = "showInComposer", skip_serializing_if = "Option::is_none")]
    pub show_in_composer: Option<bool>,
    #[serde(default, rename = "showAgentLabel", skip_serializing_if = "Option::is_none")]
    pub show_agent_label: Option<bool>,
    #[serde(
        default,
        rename = "showSidecarToolCalls",
        skip_serializing_if = "Option::is_none"
    )]
    pub show_sidecar_tool_calls: Option<bool>,
    #[serde(
        default,
        rename = "showNonSidecarToolCalls",
        skip_serializing_if = "Option::is_none"
    )]
    pub show_non_sidecar_tool_calls: Option<bool>,
    #[serde(default, rename = "showReasoning", skip_serializing_if = "Option::is_none")]
    pub show_reasoning: Option<bool>,
    #[serde(default, rename = "showSubAgentTrace", skip_serializing_if = "Option::is_none")]
    pub show_sub_agent_trace: Option<bool>,
    #[serde(default, rename = "showToolCalls", skip_serializing_if = "Option::is_none")]
    pub show_tool_calls: Option<bool>,
    #[serde(default, rename = "showToolCallResults", skip_serializing_if = "Option::is_none")]
    pub show_tool_call_results: Option<bool>,
    #[serde(default, rename = "hideToolNames", skip_serializing_if = "Option::is_none")]
    pub hide_tool_names: Option<Vec<String>>,
    #[serde(default, rename = "showWorkspacePicker", skip_serializing_if = "Option::is_none")]
    pub show_workspace_picker: Option<bool>,
    #[serde(default, rename = "showComputerMonitorPicker", skip_serializing_if = "Option::is_none")]
    pub show_computer_monitor_picker: Option<bool>,
    #[serde(default, rename = "showTaskBoardPanel", skip_serializing_if = "Option::is_none")]
    pub show_task_board_panel: Option<bool>,
    /// When true, user may pick this worker in the chat composer agent menu.
    #[serde(default, rename = "userSelectable", skip_serializing_if = "Option::is_none")]
    pub user_selectable: Option<bool>,
    /// Optional label for the chat composer agent picker (UI only).
    #[serde(default, rename = "composerLabel", skip_serializing_if = "Option::is_none")]
    pub composer_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAgentUi {
    pub show_in_composer: bool,
    pub show_agent_label: bool,
    pub show_sidecar_tool_calls: bool,
    pub show_non_sidecar_tool_calls: bool,
    pub show_reasoning: bool,
    pub show_sub_agent_trace: bool,
    pub show_tool_calls: bool,
    pub show_tool_call_results: bool,
    pub hide_tool_names: Vec<String>,
    pub show_workspace_picker: bool,
    pub show_computer_monitor_picker: bool,
    pub show_task_board_panel: bool,
    pub user_selectable: bool,
    pub composer_label: String,
    pub avatar: String,
}

fn default_composer_label(profile: &AgentProfile, role: &str, id: &str) -> String {
    if role == "supervisor" || id == "supervisor" {
        return "团队模式".into();
    }
    match id {
        "general" => "通用助手".into(),
        "coder" => "氛围编程".into(),
        "computer" => "电脑操控".into(),
        "explore" => "代码探索".into(),
        "general-worker" => "通用执行".into(),
        "research" => "深度研究".into(),
        _ => match profile {
            AgentProfile::Computer => "电脑操控".into(),
            AgentProfile::Coder => "氛围编程".into(),
            AgentProfile::Explore => "代码探索".into(),
            AgentProfile::Analyst => "深度研究".into(),
            AgentProfile::Supervisor => "团队模式".into(),
            _ => "通用助手".into(),
        },
    }
}

fn profile_defaults(profile: &AgentProfile, role: &str, id: &str) -> ResolvedAgentUi {
    let is_supervisor = role == "supervisor" || id == "supervisor";
    let is_computer = matches!(profile, AgentProfile::Computer) || id == "computer";
    let is_coder = matches!(profile, AgentProfile::Coder) || id == "coder";
    let is_research = matches!(profile, AgentProfile::Analyst) || id == "research";
    let has_task_board = !is_supervisor;
    ResolvedAgentUi {
        show_in_composer: !is_supervisor,
        show_agent_label: true,
        show_sidecar_tool_calls: false,
        show_non_sidecar_tool_calls: true,
        show_reasoning: false,
        show_sub_agent_trace: is_supervisor || is_research,
        show_tool_calls: !is_supervisor,
        show_tool_call_results: false,
        hide_tool_names: if has_task_board {
            vec!["task_board_init".into(), "task_board_patch".into(), "task_board_replace".into(), "task_board_finalize".into()]
        } else {
            vec![]
        },
        show_workspace_picker: is_coder,
        show_computer_monitor_picker: is_computer,
        show_task_board_panel: has_task_board,
        user_selectable: is_computer || is_coder || is_research || id == "general",
        composer_label: default_composer_label(profile, role, id),
        avatar: if is_supervisor {
            "supervisor".into()
        } else if is_computer {
            "computer".into()
        } else if is_coder {
            "coder".into()
        } else if matches!(profile, AgentProfile::Explore) || id == "explore" {
            "explore".into()
        } else if matches!(profile, AgentProfile::Analyst) || id == "research" {
            "research".into()
        } else {
            "general".into()
        },
    }
}

fn merge_bool(manifest: Option<bool>, base: bool) -> bool {
    manifest.unwrap_or(base)
}

fn merge_vec(manifest: Option<Vec<String>>, base: Vec<String>) -> Vec<String> {
    manifest.unwrap_or(base)
}

fn merge_str(manifest: Option<String>, base: String) -> String {
    manifest.filter(|s| !s.trim().is_empty())
        .unwrap_or(base)
}

fn merge_composer_label(
    manifest: Option<String>,
    profile: &AgentProfile,
    role: &str,
    id: &str,
) -> String {
    manifest
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_composer_label(profile, role, id))
}

/// User-visible agent label (Chinese composer label). English slug stays in `AgentDef::name` for settings only.
pub fn agent_display_label(def: &AgentDef) -> String {
    resolve_agent_ui(def).composer_label
}

pub fn resolve_agent_ui(def: &AgentDef) -> ResolvedAgentUi {
    let base = profile_defaults(&def.profile, &def.role, &def.id);
    let ui = &def.ui;
    ResolvedAgentUi {
        show_in_composer: merge_bool(ui.show_in_composer, base.show_in_composer),
        show_agent_label: merge_bool(ui.show_agent_label, base.show_agent_label),
        show_sidecar_tool_calls: merge_bool(
            ui.show_sidecar_tool_calls,
            base.show_sidecar_tool_calls,
        ),
        show_non_sidecar_tool_calls: merge_bool(
            ui.show_non_sidecar_tool_calls,
            base.show_non_sidecar_tool_calls,
        ),
        show_reasoning: merge_bool(ui.show_reasoning, base.show_reasoning),
        show_sub_agent_trace: merge_bool(ui.show_sub_agent_trace, base.show_sub_agent_trace),
        show_tool_calls: merge_bool(ui.show_tool_calls, base.show_tool_calls),
        show_tool_call_results: merge_bool(ui.show_tool_call_results, base.show_tool_call_results),
        hide_tool_names: merge_vec(ui.hide_tool_names.clone(), base.hide_tool_names),
        show_workspace_picker: merge_bool(ui.show_workspace_picker, base.show_workspace_picker),
        show_computer_monitor_picker: merge_bool(
            ui.show_computer_monitor_picker,
            base.show_computer_monitor_picker,
        ),
        show_task_board_panel: merge_bool(ui.show_task_board_panel, base.show_task_board_panel),
        user_selectable: merge_bool(ui.user_selectable, base.user_selectable),
        composer_label: merge_composer_label(
            ui.composer_label.clone(),
            &def.profile,
            &def.role,
            &def.id,
        ),
        avatar: merge_str(ui.avatar.clone(), base.avatar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AccessPolicy, AgentDef, AgentProfile};

    fn sample_def(id: &str, name: &str, profile: AgentProfile) -> AgentDef {
        AgentDef {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            role: "worker".into(),
            profile,
            default_skill_ids: vec![],
            access_policy: AccessPolicy::default(),
            builtin: true,
            enabled: true,
            tool_names: vec![],
            source: None,
            resource_files: vec![],
            allow_agents: vec![],
            config: Default::default(),
            ui: AgentUiConfig::default(),
        }
    }

    #[test]
    fn display_label_uses_chinese_not_english_slug() {
        let def = sample_def("general", "general-assistant", AgentProfile::General);
        assert_eq!(agent_display_label(&def), "通用助手");
        let coder = sample_def("coder", "vibe-coding", AgentProfile::Coder);
        assert_eq!(agent_display_label(&coder), "氛围编程");
    }
}
