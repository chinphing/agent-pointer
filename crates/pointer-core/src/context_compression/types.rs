//! Compression UI scope and stream helpers.

use super::budget::DROP_FALLBACK_KEEP_USER_TURNS;
use crate::agent_instance_scope::AgentInstanceScope;
use crate::models::{ChatStreamSender, ContextCompressionInfo, StreamEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub(crate) type StreamTx = ChatStreamSender;
/// Whether compression UI/events target the main thread or an isolated sub-agent loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionScope {
    #[default]
    Main,
    SubAgent,
}

/// Optional anchors for compression UI (main thread vs delegated sub-agent such as `explore`).
#[derive(Debug, Clone, Default)]
pub struct CompressionUiContext {
    pub scope: CompressionScope,
    /// Token reporting scope for this compression LLM call.
    pub agent_scope: Option<AgentInstanceScope>,
    /// Parent assistant message id (sub-agent trace anchoring).
    pub message_id: Option<String>,
    pub sub_agent_id: Option<String>,
    pub sub_agent_name: Option<String>,
    pub task_id: Option<String>,
}

impl CompressionUiContext {
    pub fn main(agent_scope: AgentInstanceScope) -> Self {
        Self {
            scope: CompressionScope::Main,
            agent_scope: Some(agent_scope),
            ..Self::default()
        }
    }

    pub fn sub_agent(
        agent_scope: AgentInstanceScope,
        message_id: &str,
        agent_id: &str,
        agent_name: &str,
        task_id: &str,
    ) -> Self {
        Self {
            scope: CompressionScope::SubAgent,
            agent_scope: Some(agent_scope),
            message_id: Some(message_id.to_string()),
            sub_agent_id: Some(agent_id.to_string()),
            sub_agent_name: Some(agent_name.to_string()),
            task_id: Some(task_id.to_string()),
        }
    }
}

pub(crate) fn sub_agent_compression_queue_key(
    conversation_id: &str,
    agent_instance_id: &str,
) -> String {
    format!(
        "sub:{}:{}",
        conversation_id.trim(),
        agent_instance_id.trim()
    )
}

pub(crate) fn compression_queue_key(conversation_id: &str, ui: &CompressionUiContext) -> String {
    match ui.scope {
        CompressionScope::Main => conversation_id.trim().to_string(),
        CompressionScope::SubAgent => {
            let inst = ui
                .agent_scope
                .as_ref()
                .map(|s| s.agent_instance_id.as_str())
                .unwrap_or("");
            sub_agent_compression_queue_key(conversation_id, inst)
        }
    }
}

/// Drops sub-agent pending splices when the isolated loop ends.
pub(crate) struct SubAgentPrecompressLease {
    key: String,
    live: Arc<AtomicBool>,
}

impl SubAgentPrecompressLease {
    pub(crate) fn new(key: String) -> Self {
        Self {
            key,
            live: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn live(&self) -> Arc<AtomicBool> {
        self.live.clone()
    }
}

impl Drop for SubAgentPrecompressLease {
    fn drop(&mut self) {
        self.live.store(false, Ordering::SeqCst);
        super::precompress::discard_pending_by_key(&self.key);
    }
}

pub(crate) fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn emit_ui_toast(stream: &StreamTx, conversation_id: &str, message: &str, level: &str) {
    crate::stream_broadcast::publish_stream(
        stream,
        StreamEvent::UiToast {
            conversation_id: conversation_id.to_string(),
            message: message.to_string(),
            level: level.to_string(),
        },
    );
}

pub(crate) fn emit_compression_started(
    stream: &StreamTx,
    conversation_id: &str,
    ui: &CompressionUiContext,
    insert_before_message_id: Option<String>,
) {
    let scope = match ui.scope {
        CompressionScope::Main => "main",
        CompressionScope::SubAgent => "sub_agent",
    };
    crate::stream_broadcast::publish_stream(
        stream,
        StreamEvent::ContextCompressionStarted {
            conversation_id: conversation_id.to_string(),
            scope: scope.to_string(),
            message_id: ui.message_id.clone(),
            insert_before_message_id,
            sub_agent_id: ui.sub_agent_id.clone(),
            sub_agent_name: ui.sub_agent_name.clone(),
        },
    );
}

pub(crate) fn compression_done_toast(
    ui: &CompressionUiContext,
    dropped: u32,
    _keep_users: u32,
    summary_failed: bool,
) -> (String, &'static str) {
    let level = if summary_failed { "warning" } else { "success" };
    let msg = match ui.scope {
        CompressionScope::Main => {
            if summary_failed {
                format!(
                    "摘要生成失败，已丢弃较早 {dropped} 条记录，并保留最近 {keep} 轮用户消息",
                    keep = DROP_FALLBACK_KEEP_USER_TURNS
                )
            } else {
                format!("已压缩较早 {dropped} 条对话为摘要，并保留最近对话")
            }
        }
        CompressionScope::SubAgent => {
            let name = ui
                .sub_agent_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("子 Agent");
            if summary_failed {
                format!("{name} 子任务：摘要失败，已丢弃较早 {dropped} 条记录")
            } else {
                format!("{name} 子任务：已压缩较早 {dropped} 条记录为摘要")
            }
        }
    };
    (msg, level)
}

pub(crate) fn build_compression_info(
    ui: &CompressionUiContext,
    reason: &str,
    messages_before: usize,
    messages_after: usize,
    dropped: u32,
    keep_users: u32,
) -> ContextCompressionInfo {
    ContextCompressionInfo {
        reason: reason.to_string(),
        messages_before: messages_before as u32,
        messages_after: messages_after as u32,
        dropped_count: dropped,
        keep_recent_user_turns: keep_users,
        scope: match ui.scope {
            CompressionScope::Main => "main".into(),
            CompressionScope::SubAgent => "sub_agent".into(),
        },
        sub_agent_id: ui.sub_agent_id.clone(),
        sub_agent_name: ui.sub_agent_name.clone(),
        task_id: ui.task_id.clone(),
    }
}
