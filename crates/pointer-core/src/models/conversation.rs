use serde::{Deserialize, Serialize};

use super::message::ChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    pub messages: Vec<ChatMessage>,
    #[serde(default, rename = "skillIds")]
    pub skill_ids: Vec<String>,
    /// Cumulative tool rounds for **single-agent** replies in this conversation.
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    /// Cumulative tool rounds for **Supervisor** runs (all sub-agents) in this conversation.
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
    /// Selected desktop monitor id for Computer agent (session UX). Empty/None = auto (monitor under cursor).
    #[serde(
        default,
        rename = "computerMonitorId",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_monitor_id: Option<String>,
    /// Per-conversation workspace root for coder/file tools (session UI only).
    #[serde(default, rename = "workspaceRoot", skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    /// True when the user explicitly picked `workspace_root` in the composer (not auto sandbox).
    #[serde(default, rename = "workspaceUserSet", skip_serializing_if = "is_false_bool")]
    pub workspace_user_set: bool,
    /// User cleared workspace in composer; do not inherit another session's directory.
    #[serde(
        default,
        rename = "workspaceInheritDisabled",
        skip_serializing_if = "is_false_bool"
    )]
    pub workspace_inherit_disabled: bool,
    /// Per-conversation lead worker when `agent_mode` is `single`.
    #[serde(
        default = "super::settings::default_lead_agent_id",
        rename = "leadAgentId",
        skip_serializing_if = "is_default_session_lead_agent"
    )]
    pub lead_agent_id: String,
    /// Per-conversation orchestration mode (`single` or `supervisor`).
    #[serde(
        default = "super::settings::default_agent_mode",
        rename = "agentMode",
        skip_serializing_if = "is_default_session_agent_mode"
    )]
    pub agent_mode: String,
}

fn is_default_session_lead_agent(id: &str) -> bool {
    id.trim().is_empty() || id.trim() == super::settings::default_lead_agent_id()
}

fn is_default_session_agent_mode(mode: &str) -> bool {
    mode.trim().is_empty() || mode.trim() == super::settings::default_agent_mode()
}

fn is_false_bool(v: &bool) -> bool {
    !*v
}

/// Conversation shell fields for P1 meta-only persistence (no messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(default, rename = "skillIds")]
    pub skill_ids: Vec<String>,
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
    #[serde(
        default,
        rename = "computerMonitorId",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_monitor_id: Option<String>,
    #[serde(default, rename = "workspaceRoot", skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    #[serde(default, rename = "workspaceUserSet", skip_serializing_if = "is_false_bool")]
    pub workspace_user_set: bool,
    #[serde(
        default,
        rename = "workspaceInheritDisabled",
        skip_serializing_if = "is_false_bool"
    )]
    pub workspace_inherit_disabled: bool,
    #[serde(
        default = "super::settings::default_lead_agent_id",
        rename = "leadAgentId",
        skip_serializing_if = "is_default_session_lead_agent"
    )]
    pub lead_agent_id: String,
    #[serde(
        default = "super::settings::default_agent_mode",
        rename = "agentMode",
        skip_serializing_if = "is_default_session_agent_mode"
    )]
    pub agent_mode: String,
}

impl From<&Conversation> for ConversationMeta {
    fn from(c: &Conversation) -> Self {
        Self {
            id: c.id.clone(),
            title: c.title.clone(),
            created_at: c.created_at,
            updated_at: c.updated_at,
            skill_ids: c.skill_ids.clone(),
            tool_rounds_used: c.tool_rounds_used,
            tool_rounds_used_supervisor: c.tool_rounds_used_supervisor,
            computer_monitor_id: c.computer_monitor_id.clone(),
            workspace_root: c.workspace_root.clone(),
            workspace_user_set: c.workspace_user_set,
            workspace_inherit_disabled: c.workspace_inherit_disabled,
            lead_agent_id: c.lead_agent_id.clone(),
            agent_mode: c.agent_mode.clone(),
        }
    }
}

/// Usable desktop rectangle excluding OS chrome (macOS Dock / menu bar, Windows taskbar).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorWorkArea {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// Desktop monitor descriptor for Computer agent screen selection (UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerMonitor {
    /// Stable id for picker + persistence. New ids use `xcap:{os_id}`; legacy `{left},{top},{width},{height}` still accepted.
    pub id: String,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    #[serde(default, rename = "isPrimary")]
    pub is_primary: bool,
    /// When set, window placement should use this instead of full `left`/`top`/`width`/`height`.
    #[serde(default, rename = "workArea", skip_serializing_if = "Option::is_none")]
    pub work_area: Option<MonitorWorkArea>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendChatPayload {
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, rename = "enabledSkillIds")]
    pub enabled_skill_ids: Vec<String>,
    #[serde(default, rename = "agentMode")]
    pub agent_mode: Option<String>,
    /// Session cumulative tool rounds (single-agent mode) before this user message.
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    /// Session cumulative tool rounds (Supervisor / sub-agents) before this user message.
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
    /// Workspace root for this conversation run (overrides global settings when non-empty).
    #[serde(default, rename = "workspaceRoot", skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    /// Session lead worker override for this run (`single` mode).
    #[serde(default, rename = "leadAgentId", skip_serializing_if = "Option::is_none")]
    pub lead_agent_id: Option<String>,
}

#[cfg(test)]
mod agent_trace_persistence_tests {
    use super::*;
    use crate::models::{
        AgentTrace, ChatMessage, ComputerOperationTarget, Role, SubAgentSessionUi, SubAgentToolStats,
    };
    use super::super::settings::{default_agent_mode, default_lead_agent_id};

    #[test]
    fn agent_trace_session_round_trips_in_conversation_json() {
        let trace = AgentTrace {
            id: "task-1:computer".into(),
            name: "电脑操控".into(),
            role: "worker".into(),
            status: "completed".into(),
            detail: None,
            content: None,
            depth: Some(1),
            collapsed: true,
            user_expanded: false,
            computer_target: Some(ComputerOperationTarget::External),
            session: Some(SubAgentSessionUi {
                thoughts: Some("done".into()),
                stats: SubAgentToolStats {
                    mouse_count: 2,
                    input_count: 1,
                    other_count: 3,
                    ..Default::default()
                },
                summary_line: Some(
                    "电脑操控 · 已完成 · 鼠标 2 次 · 输入 1 次 · 其他 3 次".into(),
                ),
                collapsed: true,
                ..Default::default()
            }),
        };
        let conv = Conversation {
            id: "c1".into(),
            title: "t".into(),
            created_at: 1,
            updated_at: 1,
            messages: vec![ChatMessage {
                id: "m1".into(),
                role: Role::Assistant,
                content: String::new(),
                status: "done".into(),
                created_at: 1,
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
                agent_trace: Some(vec![trace]),
                images_base64: None,
                image_slot_labels: None,
                computer_round_screen_rel_path: None,
                ui_bindings: None,
                context_state: None,
                attachments: None,
                anchor_message_id: None,
                trace_id: None,
                task_id: None,
                spawn_depth: None,
            }],
            skill_ids: vec![],
            tool_rounds_used: 0,
            tool_rounds_used_supervisor: 0,
            computer_monitor_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: default_lead_agent_id(),
            agent_mode: default_agent_mode(),
        };
        let json = serde_json::to_string(&conv).expect("serialize");
        let back: Conversation = serde_json::from_str(&json).expect("deserialize");
        let session = back.messages[0]
            .agent_trace
            .as_ref()
            .and_then(|t| t.first())
            .and_then(|t| t.session.as_ref())
            .expect("session persisted");
        assert_eq!(session.stats.mouse_count, 2);
        assert_eq!(session.stats.input_count, 1);
        assert_eq!(session.collapsed, true);
    }
}
