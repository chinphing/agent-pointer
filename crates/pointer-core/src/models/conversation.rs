use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::message::ChatMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(rename = "workspaceRoot")]
    pub workspace_root: String,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    #[serde(rename = "isPinned")]
    pub is_pinned: bool,
    #[serde(rename = "isArchived")]
    pub is_archived: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    /// Newest owned conversation activity, or `created_at` when none exists.
    #[serde(rename = "lastActivityAt")]
    pub last_activity_at: i64,
    /// Owner identity (`SSO sub` / OAuth id / `local-admin`). Empty = legacy/anonymous.
    #[serde(rename = "sessionUserId", default)]
    pub session_user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCreationResult {
    pub project: Project,
    #[serde(rename = "reusedExisting")]
    pub reused_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPage {
    pub items: Vec<Project>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<ProjectCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCursor {
    #[serde(rename = "lastActivityAt")]
    pub last_activity_at: i64,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    /// User-pinned conversations stay above unpinned ones in sidebar lists.
    #[serde(default, rename = "isPinned")]
    pub is_pinned: bool,
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
    /// Persisted project that owns this conversation. `None` is only valid
    /// before the one-time project backfill completes.
    #[serde(default, rename = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Per-conversation workspace root for coder/file tools (session UI only).
    #[serde(
        default,
        rename = "workspaceRoot",
        skip_serializing_if = "String::is_empty"
    )]
    pub workspace_root: String,
    /// True when the user explicitly picked `workspace_root` in the composer (not auto sandbox).
    #[serde(
        default,
        rename = "workspaceUserSet",
        skip_serializing_if = "is_false_bool"
    )]
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
    /// Per-conversation performance tier override (Composer picker); `None` = global default.
    #[serde(
        default,
        rename = "performanceMode",
        skip_serializing_if = "Option::is_none"
    )]
    pub performance_mode: Option<String>,
    /// Platform login user id or IM channel user / group key for this session.
    #[serde(
        default,
        rename = "sessionUserId",
        skip_serializing_if = "String::is_empty"
    )]
    pub session_user_id: String,
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
///
/// `message_count` and `preview` come from the SQLite `conversations` table and
/// are only populated by the meta-list read path (`load_metas`). When a
/// `ConversationMeta` is derived from an in-memory `Conversation` via
/// `From<&Conversation>`, they default to `0` / empty because `Conversation`
/// does not carry them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    /// User-pinned conversations stay above unpinned ones in sidebar lists.
    #[serde(default, rename = "isPinned")]
    pub is_pinned: bool,
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
    #[serde(default, rename = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(
        default,
        rename = "workspaceRoot",
        skip_serializing_if = "String::is_empty"
    )]
    pub workspace_root: String,
    #[serde(
        default,
        rename = "workspaceUserSet",
        skip_serializing_if = "is_false_bool"
    )]
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
    /// Per-conversation performance tier override (Composer picker); `None` = global default.
    #[serde(
        default,
        rename = "performanceMode",
        skip_serializing_if = "Option::is_none"
    )]
    pub performance_mode: Option<String>,
    /// Persisted message count for the conversation (DB-backed; 0 when derived
    /// from an in-memory `Conversation`).
    #[serde(default, rename = "messageCount")]
    pub message_count: u32,
    /// Short preview of the latest messages (DB-backed; empty when derived from
    /// an in-memory `Conversation`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preview: String,
    #[serde(
        default,
        rename = "sessionUserId",
        skip_serializing_if = "String::is_empty"
    )]
    pub session_user_id: String,
}

/// One real user-turn row for the in-chat navigation rail (no headings).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationOutlineItem {
    #[serde(rename = "messageId")]
    pub message_id: String,
    pub preview: String,
    /// User-marked milestone. Omitted on the wire when false.
    #[serde(default, skip_serializing_if = "is_false_bool")]
    pub milestone: bool,
}

/// One message hit inside a conversation search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSearchMatch {
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(default)]
    pub role: String,
    pub snippet: String,
}

/// Lightweight sidebar search hit (FTS message match and/or title/preview match).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSearchHit {
    pub id: String,
    pub title: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub snippet: String,
    /// Matched message id when the hit came from message body FTS; empty for title-only hits.
    #[serde(
        default,
        rename = "messageId",
        skip_serializing_if = "String::is_empty"
    )]
    pub message_id: String,
    #[serde(default, rename = "messageCount")]
    pub message_count: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub preview: String,
    /// Additional FTS hits in this conversation (primary first).
    /// Sidebar search returns the primary only; expand loads the rest.
    /// `session_search` tool caps at 5.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<ConversationSearchMatch>,
    /// Unique FTS hits (dumps skipped). Sidebar expand may list fewer after
    /// contiguous-prefix filtering. Tool: may exceed `matches.len()` when capped.
    #[serde(default, rename = "matchCount", skip_serializing_if = "is_zero_u32")]
    pub match_count: u32,
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

impl From<&Conversation> for ConversationMeta {
    fn from(c: &Conversation) -> Self {
        Self {
            id: c.id.clone(),
            title: c.title.clone(),
            created_at: c.created_at,
            updated_at: c.updated_at,
            is_pinned: c.is_pinned,
            skill_ids: c.skill_ids.clone(),
            tool_rounds_used: c.tool_rounds_used,
            tool_rounds_used_supervisor: c.tool_rounds_used_supervisor,
            computer_monitor_id: c.computer_monitor_id.clone(),
            project_id: c.project_id.clone(),
            workspace_root: c.workspace_root.clone(),
            workspace_user_set: c.workspace_user_set,
            workspace_inherit_disabled: c.workspace_inherit_disabled,
            lead_agent_id: c.lead_agent_id.clone(),
            agent_mode: c.agent_mode.clone(),
            performance_mode: c.performance_mode.clone(),
            message_count: 0,
            preview: String::new(),
            session_user_id: c.session_user_id.clone(),
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
    #[serde(default, rename = "agentSkillOverrides")]
    pub agent_skill_overrides: HashMap<String, Vec<String>>,
    #[serde(default, rename = "agentMode")]
    pub agent_mode: Option<String>,
    /// Session cumulative tool rounds (single-agent mode) before this user message.
    #[serde(default, rename = "toolRoundsUsed")]
    pub tool_rounds_used: u32,
    /// Session cumulative tool rounds (Supervisor / sub-agents) before this user message.
    #[serde(default, rename = "toolRoundsUsedSupervisor")]
    pub tool_rounds_used_supervisor: u32,
    /// Workspace root for this conversation run (overrides global settings when non-empty).
    #[serde(
        default,
        rename = "workspaceRoot",
        skip_serializing_if = "String::is_empty"
    )]
    pub workspace_root: String,
    /// When true, do not inherit another conversation's workspace (user cleared composer).
    #[serde(
        default,
        rename = "workspaceInheritDisabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub workspace_inherit_disabled: Option<bool>,
    /// Session lead worker override for this run (`single` mode).
    #[serde(
        default,
        rename = "leadAgentId",
        skip_serializing_if = "Option::is_none"
    )]
    pub lead_agent_id: Option<String>,
    /// Per-conversation performance tier override (Composer picker); unset = global default.
    #[serde(
        default,
        rename = "performanceMode",
        skip_serializing_if = "Option::is_none"
    )]
    pub performance_mode: Option<String>,
}

#[cfg(test)]
mod agent_trace_persistence_tests {
    use super::super::settings::{default_agent_mode, default_lead_agent_id};
    use super::*;
    use crate::models::{
        AgentTrace, ChatMessage, ComputerOperationTarget, Role, SubAgentSessionUi,
        SubAgentToolStats,
    };

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
            agent_instance_id: Some("instance-1".into()),
            computer_target: Some(ComputerOperationTarget::External),
            parent_tool_call_id: Some("call-run-subagent".into()),
            anchor_message_id: None,
            summary_line: None,
            task_id: Some("task-1".into()),
            agent_id: Some("computer".into()),
            search_tool_call_ids: None,
            session: Some(SubAgentSessionUi {
                thoughts: Some("done".into()),
                stats: SubAgentToolStats {
                    mouse_count: 2,
                    input_count: 1,
                    other_count: 3,
                    ..Default::default()
                },
                summary_line: Some("电脑操控 · 已完成 · 鼠标 2 次 · 输入 1 次 · 其他 3 次".into()),
                collapsed: true,
                ..Default::default()
            }),
        };
        let conv = Conversation {
            id: "c1".into(),
            title: "t".into(),
            created_at: 1,
            updated_at: 1,
            is_pinned: false,
            messages: vec![ChatMessage {
                id: "m1".into(),
                role: Role::Assistant,
                content: String::new(),
                status: "done".into(),
                created_at: 1,
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
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
            project_id: None,
            workspace_root: String::new(),
            workspace_user_set: false,
            workspace_inherit_disabled: false,
            lead_agent_id: default_lead_agent_id(),
            agent_mode: default_agent_mode(),
            performance_mode: None,
            session_user_id: String::new(),
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
        assert_eq!(
            back.messages[0].agent_trace.as_ref().unwrap()[0]
                .agent_instance_id
                .as_deref(),
            Some("instance-1")
        );
        assert!(json.contains(r#""agentInstanceId":"instance-1""#));
    }
}
