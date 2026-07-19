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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
        scoped_message_id: Option<String>,
    },
    ReasoningDelta {
        #[serde(rename = "messageId")]
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "displayLabel")]
        display_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "displaySummary")]
        display_summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
        scoped_message_id: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "outputContext")]
        output_context: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "inputHint")]
        input_hint: Option<String>,
        #[serde(rename = "inputClass")]
        input_class: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "traceId")]
        trace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "scopedMessageId")]
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
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolRoundsUsedTotal")]
        tool_rounds_used_total: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "toolRoundsUsedSupervisorTotal")]
        tool_rounds_used_supervisor_total: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none", rename = "maxToolRounds")]
        max_tool_rounds: Option<u32>,
    },
    /// Task-board trim marked earlier messages excluded from LLM context (UI patch only).
    ContextTrimApplied {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "excludedMessageIds")]
        excluded_message_ids: Vec<String>,
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
    /// Supervisor finished planning; UI may show a task checklist.
    SupervisorPlan {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "messageId")]
        message_id: String,
        tasks: Vec<SupervisorPlanTask>,
    },
    /// Task board document changed (for chat UI panel).
    TaskBoardUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "storeKey")]
        store_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "anchorMessageId")]
        anchor_message_id: Option<String>,
        document: serde_json::Value,
    },
    /// Skill catalog changed (import / reload); UI should refresh the skill list.
    SkillsUpdated {
        #[serde(rename = "conversationId")]
        conversation_id: String,
        #[serde(rename = "importedIds")]
        imported_ids: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "enabledIds")]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorPlanTask {
    pub id: String,
    pub title: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
}

/// Channel used to push [`StreamEvent`] updates to the Pointer UI (Tauri / web SSE).
pub type ChatStreamSender = tokio::sync::mpsc::UnboundedSender<StreamEvent>;

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
}
