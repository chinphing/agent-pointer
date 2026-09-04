use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, rename = "durationMs")]
    pub duration_ms: Option<u64>,
    #[serde(default, rename = "riskLevel")]
    pub risk_level: Option<String>,
    /// UI-only Chinese label (not sent to the LLM).
    #[serde(
        default,
        rename = "displayLabel",
        skip_serializing_if = "Option::is_none"
    )]
    pub display_label: Option<String>,
    /// UI-only short parameter summary (not sent to the LLM).
    #[serde(
        default,
        rename = "displaySummary",
        skip_serializing_if = "Option::is_none"
    )]
    pub display_summary: Option<String>,
}

/// Sub-agent collapsed-header counters (frontend-only; persisted with conversations).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentToolStats {
    #[serde(default)]
    pub search_count: u32,
    #[serde(default)]
    pub read_count: u32,
    #[serde(default)]
    pub write_count: u32,
    #[serde(default)]
    pub terminal_count: u32,
    #[serde(default)]
    pub web_search_count: u32,
    #[serde(default)]
    pub mouse_count: u32,
    #[serde(default)]
    pub input_count: u32,
    #[serde(default)]
    pub other_count: u32,
    #[serde(default)]
    pub skill_count: u32,
    #[serde(default)]
    pub media_count: u32,
}

/// Sub-agent streaming UI state (tool calls, thoughts, collapsed summary); not sent to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentSessionUi {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(
        default,
        rename = "toolNamePreview",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_name_preview: Option<String>,
    #[serde(
        default,
        rename = "responseTextDraft",
        skip_serializing_if = "Option::is_none"
    )]
    pub response_text_draft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(
        default,
        rename = "rawContent",
        skip_serializing_if = "Option::is_none"
    )]
    pub raw_content: Option<String>,
    #[serde(default, rename = "contentStreaming")]
    pub content_streaming: bool,
    #[serde(default, rename = "toolCalls", skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub stats: SubAgentToolStats,
    #[serde(
        default,
        rename = "summaryLine",
        skip_serializing_if = "Option::is_none"
    )]
    pub summary_line: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, rename = "userExpanded")]
    pub user_expanded: bool,
}

/// Whether a delegated `computer` sub-task automates Pointer itself or external apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerOperationTarget {
    #[serde(rename = "self")]
    SelfApp,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    /// SpawnId (`agent_instance_id`). Legacy rows may still use `task:agent`.
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    /// UI indentation: 0 = top-level (lead / supervisor), 1 = delegated sub-agent step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Legacy nested UI session; superseded by scoped child `ChatMessage` rows. Read-only for old data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SubAgentSessionUi>,
    /// Collapsed summary header (persisted index UI).
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, rename = "userExpanded")]
    pub user_expanded: bool,
    /// Runtime child invocation UUID used to isolate scoped rows with reused task IDs.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agentInstanceId"
    )]
    pub agent_instance_id: Option<String>,
    /// Set on `run_subagent` → `computer` traces; controls dock-bar shrink in the desktop client.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "computerTarget"
    )]
    pub computer_target: Option<ComputerOperationTarget>,
    /// Parent assistant `run_subagent` tool-call id; UI nests the sub-agent frame under that row.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "parentToolCallId"
    )]
    pub parent_tool_call_id: Option<String>,
    /// The `ChatMessage.id` that owns this trace's scoped child messages (for nested
    /// sub-agents this differs from the lead anchor; see [`AgentTrace::parent_tool_call_id`]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "anchorMessageId"
    )]
    pub anchor_message_id: Option<String>,
    /// Persisted collapsed summary for stub UI when scoped rows are not hydrated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "summaryLine"
    )]
    pub summary_line: Option<String>,
    /// Logical task id (explicit; do not parse from `id`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "taskId"
    )]
    pub task_id: Option<String>,
    /// Worker agent id (explore / coder / self / …).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agentId"
    )]
    pub agent_id: Option<String>,
    /// Tool-call ids in this spawn (stub search pin after scoped rows are evicted).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "searchToolCallIds"
    )]
    pub search_tool_call_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUiBindings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_board_anchor: Option<bool>,
    /// When set, the chat bubble shows this string instead of full `content`.
    /// Full `content` still goes to the LLM (context in, summary out for UI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubble_text: Option<String>,
    /// Host-generated row kind (e.g. `idle_job_push`). Not model-facing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_kind: Option<String>,
}

impl MessageUiBindings {
    pub fn idle_job_push_bubble(bubble_text: impl Into<String>) -> Self {
        Self {
            task_board_anchor: None,
            bubble_text: Some(bubble_text.into()),
            host_kind: Some("idle_job_push".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcludedReason {
    ContextCompression,
    TaskBoardTrim,
    /// Planner UI shell row: visible in transcript, omitted from lead LLM requests.
    PlannerUiShell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageContextState {
    pub included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excluded_reason: Option<ExcludedReason>,
}

impl Default for MessageContextState {
    fn default() -> Self {
        Self {
            included: true,
            excluded_reason: None,
        }
    }
}

/// Whether this row belongs to a delegated sub-agent transcript (not lead timeline / context).
pub fn is_scoped_sub_message(msg: &ChatMessage) -> bool {
    msg.anchor_message_id
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: Role,
    #[serde(default)]
    pub content: String,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(default, rename = "toolCalls")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, rename = "toolCallId")]
    pub tool_call_id: Option<String>,
    /// Registry name for `role: tool` rows (`session_search`, `file_read`, …).
    #[serde(default, rename = "toolName", skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, rename = "errorMessage")]
    pub error_message: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    /// User-visible reasoning summary from the model’s last structured turn (`thoughts` in JSON, or legacy XML).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<String>,
    /// Short title from the model’s last structured turn (`headline` in JSON, or legacy XML).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, rename = "rawContent")]
    pub raw_content: Option<String>,
    /// Ephemeral debug / host-verify side channel. **Not** model context, **not**
    /// streamed to the UI, and stripped from conversation-store payloads (large
    /// dumps historically made `load_conversation_messages` very slow).
    #[serde(
        default,
        rename = "toolRawOutput",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_raw_output: Option<String>,
    #[serde(default, rename = "agentId")]
    pub agent_id: Option<String>,
    /// Runtime agent launch UUID (one per lead / sub-agent invocation).
    #[serde(
        default,
        rename = "agentInstanceId",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_instance_id: Option<String>,
    #[serde(default, rename = "agentName")]
    pub agent_name: Option<String>,
    #[serde(default, rename = "agentTrace")]
    pub agent_trace: Option<Vec<AgentTrace>>,
    /// PNG (or other) images as raw base64 payloads for vision APIs. Serialized for the UI only when
    /// present; ephemeral computer screen inject uses this without persisting to conversation files.
    #[serde(
        default,
        rename = "imagesBase64",
        skip_serializing_if = "Option::is_none"
    )]
    pub images_base64: Option<Vec<String>>,
    /// Slot labels prepended in the API request immediately before each `images_base64` entry (same length).
    #[serde(
        default,
        rename = "imageSlotLabels",
        skip_serializing_if = "Option::is_none"
    )]
    pub image_slot_labels: Option<Vec<String>>,
    /// Path relative to app `computer-captures/` for this turn’s annotated JPEG (lazy UI load); serialized when set.
    #[serde(
        default,
        rename = "computerRoundScreenRelPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub computer_round_screen_rel_path: Option<String>,
    /// UI mount hints (e.g. TaskBoard anchor); persisted with conversation.
    #[serde(
        default,
        rename = "uiBindings",
        skip_serializing_if = "Option::is_none"
    )]
    pub ui_bindings: Option<MessageUiBindings>,
    /// Whether this message is included in LLM context.
    #[serde(
        default,
        rename = "contextState",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_state: Option<MessageContextState>,
    /// User-attached files/images (metadata persisted; base64 wire-only via `contentBase64`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<MediaAttachment>>,
    /// Parent lead assistant message id (scoped sub-agent transcript rows).
    #[serde(
        default,
        rename = "anchorMessageId",
        skip_serializing_if = "Option::is_none"
    )]
    pub anchor_message_id: Option<String>,
    /// Stable sub-task trace id; self-forks include a unique instance segment.
    #[serde(default, rename = "traceId", skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(
        default,
        rename = "spawnDepth",
        skip_serializing_if = "Option::is_none"
    )]
    pub spawn_depth: Option<u32>,
}

impl ChatMessage {
    /// JSON payload for SQLite `messages.payload`. Always omits `toolRawOutput`
    /// so skill/terminal dumps never bloat the conversation store.
    pub fn to_store_payload_json(&self) -> Result<String, serde_json::Error> {
        if self.tool_raw_output.is_none() {
            return serde_json::to_string(self);
        }
        // Avoid cloning the (possibly multi‑MB) dump: take via Value and drop the key.
        let mut value = serde_json::to_value(self)?;
        if let Some(obj) = value.as_object_mut() {
            obj.remove("toolRawOutput");
        }
        serde_json::to_string(&value)
    }

    /// Drop persisted/legacy `toolRawOutput` after load. Returns whether the
    /// field was present (caller may rewrite the row).
    pub fn strip_tool_raw_output(&mut self) -> bool {
        self.tool_raw_output.take().is_some_and(|s| !s.is_empty())
    }

    /// Minimal user-text message for trigger sources (webhook / cron / API)
    /// that supply a plain-text prompt without full UI metadata. Fills
    /// required bookkeeping fields with sensible defaults.
    pub fn user_text(content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: Role::User,
            content: content.into(),
            status: "done".into(),
            created_at: chrono::Utc::now().timestamp_millis(),
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
            agent_trace: None,
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
        }
    }
}

/// User message attachment (Composer / channels).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAttachment {
    pub id: String,
    /// `image`, `document`, `audio`, `file`
    pub kind: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(default, rename = "sizeBytes")]
    pub size_bytes: u64,
    #[serde(
        default,
        rename = "storageRelPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub storage_rel_path: Option<String>,
    /// Wire-only payload; stripped before conversation persist.
    #[serde(
        default,
        rename = "contentBase64",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_base64: Option<String>,
    #[serde(
        default,
        rename = "derivedText",
        skip_serializing_if = "Option::is_none"
    )]
    pub derived_text: Option<String>,
    /// Absolute local path for assistant reply `MEDIA:` preview in App UI.
    #[serde(
        default,
        rename = "localAbsPath",
        skip_serializing_if = "Option::is_none"
    )]
    pub local_abs_path: Option<String>,
    /// OSS HTTPS URL for video attachments (Composer upload).
    #[serde(default, rename = "remoteUrl", skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// OSS object key for re-signing or cleanup (optional).
    #[serde(
        default,
        rename = "ossObjectKey",
        skip_serializing_if = "Option::is_none"
    )]
    pub oss_object_key: Option<String>,
}

/// Annotated desktop screenshot for UI preview (same style as model vision inject).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerAnnotatedPreview {
    #[serde(rename = "imageBase64")]
    pub image_base64: String,
    /// `image/jpeg` or `image/png` for UI `data:` URLs.
    #[serde(rename = "imageMime", default = "default_computer_preview_mime")]
    pub image_mime: String,
    pub caption: String,
}

pub fn default_computer_preview_mime() -> String {
    "image/jpeg".to_string()
}

/// Chat attachment bytes for UI bubble reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMediaPreview {
    pub data_base64: String,
    pub mime_type: String,
    pub file_name: String,
}

#[cfg(test)]
mod tool_raw_output_store_tests {
    use super::*;

    #[test]
    fn store_payload_omits_tool_raw_output() {
        let mut msg = ChatMessage::user_text("hi");
        msg.role = Role::Assistant;
        msg.tool_raw_output = Some("x".repeat(8_000));
        let json = msg.to_store_payload_json().expect("serialize");
        assert!(!json.contains("toolRawOutput"));
        assert!(!json.contains(&"x".repeat(32)));
    }

    #[test]
    fn strip_tool_raw_output_clears_nonempty() {
        let mut msg = ChatMessage::user_text("hi");
        msg.tool_raw_output = Some("dump".into());
        assert!(msg.strip_tool_raw_output());
        assert!(msg.tool_raw_output.is_none());
        assert!(!msg.strip_tool_raw_output());
    }
}
