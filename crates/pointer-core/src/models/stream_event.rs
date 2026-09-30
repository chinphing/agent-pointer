use serde::{Deserialize, Serialize};

use super::conversation::ComputerMonitor;
use super::message::{AgentTrace, ChatMessage, MediaAttachment, ToolCall};

/// Metadata emitted when context compression replaces older turns with a summary.
#[derive(Debug, Clone, Serialize)]
pub struct ContextCompressionInfo {
    /// `budget` or `tool_limit`
    pub reason: String,
    #[serde(rename = "messagesBefore")]
    pub messages_before: u32,
    #[serde(rename = "messagesAfter")]
    pub messages_after: u32,
    #[serde(rename = "droppedCount")]
    pub dropped_count: u32,
    #[serde(rename = "keepRecentUserTurns")]
    pub keep_recent_user_turns: u32,
    /// `main` or `sub_agent`
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "subAgentId")]
    pub sub_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "subAgentName")]
    pub sub_agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "taskId")]
    pub task_id: Option<String>,
}

/// Cited source entry for web-search stream UI events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSourceEntry {
    pub index: u32,
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundJobView {
    pub job_id: String,
    pub status: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

/// Frontend stream event payload (mirrors src/types/chat.ts StreamEvent)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "conversationId")]
        conversation_id: String,
    },
    /// Scoped sub-agent assistant round begins (child row in conversation store).
    SubMessageStart {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "anchorMessageId")]
        anchor_message_id: String,
        #[serde(rename = "scopedMessageId")]
        scoped_message_id: String,
        #[serde(rename = "traceId")]
        trace_id: String,
        #[serde(rename = "taskId")]
        task_id: String,
        #[serde(rename = "spawnDepth")]
        spawn_depth: u32,
        #[serde(rename = "agentInstanceId")]
        agent_instance_id: String,
    },
    Delta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
    },
    RawContentDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    ReasoningDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    /// Progressive `thoughts` / `headline` / `tool_name` / `response` body (`tool_args.text`) from partial JSON repair while streaming.
    AssistantJsonPartial {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        headline: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolName")]
        tool_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "responseText")]
        response_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    AgentStep {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "agent")]
        agent: AgentTrace,
    },
    ToolCallStart {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCall")]
        tool_call: ToolCall,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    ToolCallArgsDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "argsDelta")]
        args_delta: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    ToolCallStatus {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "durationMs")]
        duration_ms: Option<u64>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "displayLabel"
        )]
        display_label: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "displaySummary"
        )]
        display_summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    TerminalOutputDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "output")]
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    ConsoleOutputDelta {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "workspaceRoot")]
        workspace_root: String,
        cwd: String,
        output: String,
    },
    ConsoleSessionExited {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "workspaceRoot")]
        workspace_root: String,
        cwd: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "exitCode")]
        exit_code: Option<i32>,
    },
    TerminalNeedsInput {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        command: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "outputContext"
        )]
        output_context: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "inputHint")]
        input_hint: Option<String>,
        #[serde(rename = "inputClass")]
        input_class: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    WebSearchOutputDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    WebSearchSourcesReady {
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        sources: Vec<WebSearchSourceEntry>,
        #[serde(rename = "searchCount")]
        search_count: u32,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
    },
    MessageEnd {
        #[serde(rename = "messageId")]
        message_id: String,
        /// 与持久化助手消息对齐的最终正文（已去掉 XML 工具块等）
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "rawContent")]
        raw_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolRawOutput")]
        tool_raw_output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thoughts: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        headline: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "scopedMessageId"
        )]
        scoped_message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<MediaAttachment>>,
    },
    /// Synthetic user row so the model (and UI history) see recovery instructions mid-run.
    InjectedUserMessage {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<MediaAttachment>>,
        /// UI mount / bubble summary; full `content` is for the model.
        #[serde(
            default,
            rename = "uiBindings",
            skip_serializing_if = "Option::is_none"
        )]
        ui_bindings: Option<super::message::MessageUiBindings>,
    },
    /// User message attachments processed (ASR, storage path, etc.).
    UserMessageAttachmentsUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        attachments: Vec<MediaAttachment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    /// IM `/new` or idle reset created a new desktop sidebar row for the same IM thread.
    ImSessionForked {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "baseConversationId")]
        base_conversation_id: String,
        title: String,
        #[serde(rename = "sessionEpoch")]
        session_epoch: u32,
        #[serde(rename = "leadAgentId")]
        lead_agent_id: String,
        #[serde(rename = "agentMode")]
        agent_mode: String,
    },
    /// IM session agent / mode changed (mirror sidebar + Composer).
    ImSessionAgentChanged {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "baseConversationId")]
        base_conversation_id: String,
        #[serde(rename = "leadAgentId")]
        lead_agent_id: String,
        #[serde(rename = "agentMode")]
        agent_mode: String,
    },
    /// Short assistant-role line in the thread (e.g. desktop capture status); not from the model.
    InjectedAssistantMessage {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
    },
    /// Replace `content` of an existing [`InjectedAssistantMessage`] with the same `message_id`.
    InjectedAssistantMessageUpdate {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        content: String,
    },
    Error {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "messageId")]
        message_id: Option<String>,
        message: String,
    },
    Done {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(
            skip_serializing_if = "Option::is_none",
            rename = "toolRoundsUsedTotal"
        )]
        tool_rounds_used_total: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "maxToolRounds")]
        max_tool_rounds: Option<u32>,
        /// 本轮 run_chat 的真实开始时间（epoch ms）。UI 用它计算"工作耗时"，
        /// 排除前端 dispatch 排队/网络传输；历史会话无此值时回退消息时间戳。
        #[serde(skip_serializing_if = "Option::is_none", rename = "startedAtMs")]
        started_at_ms: Option<i64>,
        /// 本轮 run_chat 发出 Done 的时间（epoch ms）。
        #[serde(skip_serializing_if = "Option::is_none", rename = "finishedAtMs")]
        finished_at_ms: Option<i64>,
        /// Queued + running jobs for this conversation. Composer stop / sidebar
        /// spinner follow this, not the dispatcher queue.
        #[serde(
            skip_serializing_if = "Option::is_none",
            rename = "backgroundRunningCount"
        )]
        background_running_count: Option<u32>,
    },
    /// Task-board trim marked earlier messages excluded from LLM context (UI patch only).
    ContextTrimApplied {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "excludedMessageIds")]
        excluded_message_ids: Vec<String>,
    },
    /// Ephemeral: compression LLM is running (UI marker only; not persisted).
    ContextCompressionStarted {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        /// `main` or `sub_agent`
        scope: String,
        #[serde(skip_serializing_if = "Option::is_none", rename = "messageId")]
        message_id: Option<String>,
        /// First kept message after the summary split (UI marker sits just before this).
        #[serde(
            skip_serializing_if = "Option::is_none",
            rename = "insertBeforeMessageId"
        )]
        insert_before_message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "subAgentId")]
        sub_agent_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "subAgentName")]
        sub_agent_name: Option<String>,
    },
    /// Main-thread context compression: soft-exclude prefix + insert summary user row.
    ContextCompressionApplied {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "excludedMessageIds")]
        excluded_message_ids: Vec<String>,
        #[serde(rename = "insertBeforeMessageId")]
        insert_before_message_id: String,
        #[serde(rename = "summaryMessage")]
        summary_message: ChatMessage,
        compression: ContextCompressionInfo,
    },
    /// Sub-agent local history was compressed; main thread messages are unchanged.
    ContextCompressed {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        compression: ContextCompressionInfo,
    },
    ToolRoundsExhausted {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "maxRounds")]
        max_rounds: u32,
        message: String,
        #[serde(rename = "willRetryAfterCompress")]
        will_retry_after_compress: bool,
    },
    /// Ephemeral UI hint only (not persisted, not sent to the model).
    UiToast {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        message: String,
        /// e.g. `success`, `error`, `warning`
        level: String,
    },
    /// IM DM pairing code issued; UI may prompt admin approval (no continuous idle poll).
    ChannelPairingPending {
        channel: String,
        #[serde(rename = "accountId")]
        account_id: String,
        code: String,
        #[serde(rename = "senderId")]
        sender_id: String,
        /// Unix seconds.
        #[serde(rename = "issuedAt")]
        issued_at: i64,
    },
    /// Annotated screen for a specific assistant message (this LLM round’s inject).
    AssistantRoundScreen {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        /// Path relative to `{data_dir}/PointerApp/computer-captures/` (annotated PNG).
        #[serde(rename = "annotatedRelPath")]
        annotated_rel_path: String,
    },
    /// Task board document changed (for chat UI panel).
    TaskBoardUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "storeKey")]
        store_key: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "anchorMessageId"
        )]
        anchor_message_id: Option<String>,
        document: serde_json::Value,
    },
    /// Skill catalog changed (import / reload); UI should refresh the skill list.
    SkillsUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "importedIds")]
        imported_ids: Vec<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "enabledIds"
        )]
        enabled_ids: Option<Vec<String>>,
    },
    /// Conversation workspace root changed mid-run (e.g. general → coder delegation).
    WorkspaceUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "workspaceRoot")]
        workspace_root: String,
        #[serde(rename = "isEphemeralSandbox")]
        is_ephemeral_sandbox: bool,
    },
    /// Sub-agent computer delegation blocked until the user picks a monitor.
    ComputerMonitorPickRequired {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        monitors: Vec<ComputerMonitor>,
    },
    /// Monitor selection applied (auto single-monitor or user pick).
    ComputerMonitorUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "monitorId")]
        monitor_id: Option<String>,
    },
    /// Background job occupancy for this conversation (sidebar spinner; not the lead turn).
    /// `jobs` includes nested background terminals spawned inside a sub-agent.
    BackgroundJobs {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "runningCount")]
        running_count: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        jobs: Vec<BackgroundJobView>,
    },
}

fn nonempty_id(id: &str) -> Option<&str> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

impl StreamEvent {
    /// Conversation id carried on the event payload (empty treated as absent).
    /// High-frequency deltas omit this field; SSE routing uses [`ChatStreamSender`].
    pub fn conversation_id_for_sse(&self) -> Option<&str> {
        match self {
            Self::MessageStart {
                conversation_id, ..
            }
            | Self::SubMessageStart {
                conversation_id, ..
            }
            | Self::InjectedUserMessage {
                conversation_id, ..
            }
            | Self::UserMessageAttachmentsUpdated {
                conversation_id, ..
            }
            | Self::ImSessionForked {
                conversation_id, ..
            }
            | Self::ImSessionAgentChanged {
                conversation_id, ..
            }
            | Self::InjectedAssistantMessage {
                conversation_id, ..
            }
            | Self::InjectedAssistantMessageUpdate {
                conversation_id, ..
            }
            | Self::Error {
                conversation_id, ..
            }
            | Self::Done {
                conversation_id, ..
            }
            | Self::ContextTrimApplied {
                conversation_id, ..
            }
            | Self::ContextCompressionStarted {
                conversation_id, ..
            }
            | Self::ContextCompressionApplied {
                conversation_id, ..
            }
            | Self::ContextCompressed {
                conversation_id, ..
            }
            | Self::ToolRoundsExhausted {
                conversation_id, ..
            }
            | Self::UiToast {
                conversation_id, ..
            }
            | Self::AssistantRoundScreen {
                conversation_id, ..
            }
            | Self::TaskBoardUpdated {
                conversation_id, ..
            }
            | Self::SkillsUpdated {
                conversation_id, ..
            }
            | Self::WorkspaceUpdated {
                conversation_id, ..
            }
            | Self::ComputerMonitorPickRequired {
                conversation_id, ..
            }
            | Self::ComputerMonitorUpdated {
                conversation_id, ..
            }
            | Self::BackgroundJobs {
                conversation_id, ..
            } => nonempty_id(conversation_id),
            Self::Delta { .. }
            | Self::RawContentDelta { .. }
            | Self::ReasoningDelta { .. }
            | Self::AssistantJsonPartial { .. }
            | Self::AgentStep { .. }
            | Self::ToolCallStart { .. }
            | Self::ToolCallArgsDelta { .. }
            | Self::ToolCallStatus { .. }
            | Self::TerminalOutputDelta { .. }
            | Self::ConsoleOutputDelta { .. }
            | Self::ConsoleSessionExited { .. }
            | Self::TerminalNeedsInput { .. }
            | Self::WebSearchOutputDelta { .. }
            | Self::WebSearchSourcesReady { .. }
            | Self::MessageEnd { .. }
            | Self::ChannelPairingPending { .. } => None,
        }
    }
}

/// Per-run chat UI sink. Carries conversation/user ids so SSE can route
/// high-frequency events that do not embed `conversationId`.
#[derive(Clone, Debug)]
pub struct ChatStreamSender {
    tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    conversation_id: String,
    session_user_id: String,
}

impl ChatStreamSender {
    pub fn pair(
        conversation_id: impl Into<String>,
        session_user_id: impl Into<String>,
    ) -> (Self, tokio::sync::mpsc::UnboundedReceiver<StreamEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Self {
                tx,
                conversation_id: conversation_id.into(),
                session_user_id: session_user_id.into(),
            },
            rx,
        )
    }

    /// Sender whose receiver is dropped; `send` fails but broadcast still runs.
    pub fn unbound(conversation_id: impl Into<String>, session_user_id: impl Into<String>) -> Self {
        Self::pair(conversation_id, session_user_id).0
    }

    pub fn conversation_id(&self) -> &str {
        self.conversation_id.trim()
    }

    pub fn session_user_id(&self) -> &str {
        self.session_user_id.trim()
    }

    pub fn send(
        &self,
        ev: StreamEvent,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<StreamEvent>> {
        self.tx.send(ev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_message_start_serializes_agent_instance_id() {
        let event = StreamEvent::SubMessageStart {
            conversation_id: "conversation".into(),
            anchor_message_id: "anchor".into(),
            scoped_message_id: "child".into(),
            trace_id: "task:explore".into(),
            task_id: "task".into(),
            spawn_depth: 1,
            agent_instance_id: "instance-current".into(),
        };

        let value = serde_json::to_value(event).expect("serialize");

        assert_eq!(value["agentInstanceId"], "instance-current");
    }

    #[test]
    fn error_event_serializes_conversation_id() {
        let ev = StreamEvent::Error {
            conversation_id: "conv-a".into(),
            message_id: None,
            message: "boom".into(),
        };
        let v = serde_json::to_value(&ev).expect("serialize");
        assert_eq!(v["kind"], "error");
        assert_eq!(v["conversationId"], "conv-a");
        assert_eq!(v["message"], "boom");
        assert!(v.get("messageId").is_none());
    }

    #[test]
    fn background_jobs_serializes_camel_case() {
        let ev = StreamEvent::BackgroundJobs {
            conversation_id: "conv-a".into(),
            running_count: 3,
            jobs: vec![BackgroundJobView {
                job_id: "job_1".into(),
                status: "running".into(),
                kind: "terminal".into(),
                title: Some("python scrape.py".into()),
                agent_id: None,
            }],
        };
        let v = serde_json::to_value(&ev).expect("serialize");
        assert_eq!(v["kind"], "background_jobs");
        assert_eq!(v["conversationId"], "conv-a");
        assert_eq!(v["runningCount"], 3);
        assert_eq!(v["jobs"][0]["title"], "python scrape.py");
        assert_eq!(v["jobs"][0]["kind"], "terminal");
    }

    #[test]
    fn done_serializes_background_running_count() {
        let ev = StreamEvent::Done {
            conversation_id: "conv-a".into(),
            tool_rounds_used_total: None,
            max_tool_rounds: None,
            started_at_ms: None,
            finished_at_ms: None,
            background_running_count: Some(0),
        };
        let v = serde_json::to_value(&ev).expect("serialize");
        assert_eq!(v["kind"], "done");
        assert_eq!(v["conversationId"], "conv-a");
        assert_eq!(v["backgroundRunningCount"], 0);
    }
}
