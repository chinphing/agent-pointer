//! Per-agent chat UI visibility defaults (merged with AGENT.md `ui` block).

use super::{AgentDef, AgentProfile};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUiConfig {
    #[serde(default, rename = "showInComposer", skip_serializing_if = "Option::is_none")]
    pub show_in_composer: Option<bool>,
    #[serde(default, rename = "showAgentLabel", skip_serializing_if = "Option::is_none")]
    pub show_agent_label: Option<bool>,
    #[serde(default, rename = "showThoughts", skip_serializing_if = "Option::is_none")]
    pub show_thoughts: Option<bool>,
    #[serde(default, rename = "showHeadline", skip_serializing_if = "Option::is_none")]
    pub show_headline: Option<bool>,
    #[serde(default, rename = "showSubAgentTrace", skip_serializing_if = "Option::is_none")]
    pub show_sub_agent_trace: Option<bool>,
    #[serde(default, rename = "showToolCalls", skip_serializing_if = "Option::is_none")]
    pub show_tool_calls: Option<bool>,
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
    pub show_thoughts: bool,
    pub show_headline: bool,
    pub show_sub_agent_trace: bool,
    pub show_tool_calls: bool,
    pub hide_tool_names: Vec<String>,
    pub show_workspace_picker: bool,
    pub show_computer_monitor_picker: bool,
    pub show_task_board_panel: bool,
    pub user_selectable: bool,
    pub composer_label: String,
    pub avatar: String,
}

fn profile_defaults(profile: &AgentProfile, role: &str, id: &str) -> ResolvedAgentUi {
    let is_supervisor = role == "supervisor" || id == "supervisor";
    let is_computer = matches!(profile, AgentProfile::Computer) || id == "computer";
    let is_coder = matches!(profile, AgentProfile::Coder) || id == "coder";
    let has_task_board = !is_supervisor;
    ResolvedAgentUi {
        show_in_composer: !is_supervisor,
        show_agent_label: true,
        show_thoughts: true,
        show_headline: true,
        show_sub_agent_trace: is_supervisor,
        show_tool_calls: !is_supervisor,
        hide_tool_names: if has_task_board {
            vec!["task_board".into(), "task_board:patch".into()]
        } else {
            vec![]
        },
        show_workspace_picker: is_coder,
        show_computer_monitor_picker: is_computer,
        show_task_board_panel: has_task_board,
        user_selectable: false,
        composer_label: String::new(),
        avatar: if is_supervisor {
            "supervisor".into()
        } else if is_computer {
            "computer".into()
        } else if is_coder {
            "coder".into()
        } else if matches!(profile, AgentProfile::Explore) || id == "explore" {
            "explore".into()
        } else {
            "default".into()
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

fn merge_composer_label(manifest: Option<String>, agent_name: &str) -> String {
    manifest
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| agent_name.to_string())
}

pub fn resolve_agent_ui(def: &AgentDef) -> ResolvedAgentUi {
    let base = profile_defaults(&def.profile, &def.role, &def.id);
    let ui = &def.ui;
    ResolvedAgentUi {
        show_in_composer: merge_bool(ui.show_in_composer, base.show_in_composer),
        show_agent_label: merge_bool(ui.show_agent_label, base.show_agent_label),
        show_thoughts: merge_bool(ui.show_thoughts, base.show_thoughts),
        show_headline: merge_bool(ui.show_headline, base.show_headline),
        show_sub_agent_trace: merge_bool(ui.show_sub_agent_trace, base.show_sub_agent_trace),
        show_tool_calls: merge_bool(ui.show_tool_calls, base.show_tool_calls),
        hide_tool_names: merge_vec(ui.hide_tool_names.clone(), base.hide_tool_names),
        show_workspace_picker: merge_bool(ui.show_workspace_picker, base.show_workspace_picker),
        show_computer_monitor_picker: merge_bool(
            ui.show_computer_monitor_picker,
            base.show_computer_monitor_picker,
        ),
        show_task_board_panel: merge_bool(ui.show_task_board_panel, base.show_task_board_panel),
        user_selectable: merge_bool(ui.user_selectable, base.user_selectable),
        composer_label: merge_composer_label(ui.composer_label.clone(), &def.name),
        avatar: merge_str(ui.avatar.clone(), base.avatar),
    }
}
