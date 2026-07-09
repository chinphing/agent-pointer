//! Unified trigger request schema and run handle.
//!
//! Every trigger source (Tauri IPC, HTTP Runs API, webhook, cron, IM channel,
//! internal event) normalizes into a [`TriggerRequest`] before reaching
//! [`crate::dispatcher::RunDispatcher::dispatch`]. This mirrors the openclaw
//! `agent` RPC params (idempotency key, lane, trigger kind) and the hermes
//! `MessageEvent` normalization pattern.

use serde::{Deserialize, Serialize};

use crate::models::ChatMessage;
use crate::web_request_auth::WebSessionAuth;

/// Origin of a run. Used for observability logs, hook filtering, and the
/// `runs` table `trigger_source` column. Keep variants in sync with the
/// `TriggerSource` serialization (snake_case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    /// Tauri IPC `send_chat` (desktop client).
    Ipc,
    /// HTTP `POST /api/runs` (web client / external REST caller).
    HttpRuns,
    /// Generic inbound webhook (`POST /api/webhooks/:source`).
    Webhook,
    /// Scheduled cron job.
    Cron,
    /// IM channel inbound message (Feishu / DingTalk / WeCom / Weixin).
    Im,
    /// Internal background trigger (curator / memory review / boot checklist).
    Internal,
}

impl TriggerSource {
    /// Runs that execute without an interactive browser session (webhook, cron, IM).
    pub fn is_headless_automation(self) -> bool {
        matches!(self, Self::Webhook | Self::Cron | Self::Im)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerSource::Ipc => "ipc",
            TriggerSource::HttpRuns => "http_runs",
            TriggerSource::Webhook => "webhook",
            TriggerSource::Cron => "cron",
            TriggerSource::Im => "im",
            TriggerSource::Internal => "internal",
        }
    }
}

impl std::fmt::Display for TriggerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source-specific metadata stored alongside the run. Free-form JSON so each
/// trigger source can attach its own fields (cron expr, webhook source id,
/// inbound message id, ...).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerMeta {
    /// Cron job id (when `source == Cron`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    /// Webhook source slug (when `source == Webhook`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_source: Option<String>,
    /// IM inbound dedup id (when `source == Im`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound_id: Option<String>,
    /// Internal trigger label (when `source == Internal`), e.g. `memory_review`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_label: Option<String>,
    /// Catch-all for future sources / extra context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

impl TriggerMeta {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Where the final assistant reply should be delivered after the run. IM
/// channels set this so the dispatcher can call the channel outbound adapter
/// from a single `on_run_finished` site (Phase 3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeliverTarget {
    /// No external delivery; caller reads events from the bus / runs API.
    None,
    /// IM channel reply target (channel + account + conversation key).
    Im {
        channel: String,
        account: String,
        conversation_key: String,
    },
    /// HTTP callback URL (future: webhook result delivery).
    #[allow(dead_code)]
    Webhook { url: String },
}

impl Default for DeliverTarget {
    fn default() -> Self {
        DeliverTarget::None
    }
}

/// Unified run request. All trigger sources construct one of these.
///
/// `messages` follows the existing `run_chat` contract: the caller supplies
/// the full in-memory history for the turn (including the new user message as
/// the last entry). The dispatcher does not pull pending user messages from a
/// queue; that responsibility stays with the trigger source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerRequest {
    /// Caller-provided run id. When `None`, the dispatcher generates a uuid v4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Idempotency key. When set and an existing run has the same key, the
    /// dispatcher returns the existing run handle instead of starting a new
    /// run (borrowed from openclaw `idempotencyKey`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Conversation id. When `None`, the dispatcher derives one from the
    /// trigger source + a stable key (e.g. IM session key). Phase 1 callers
    /// always supply one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub trigger_source: TriggerSource,
    #[serde(default)]
    pub trigger_meta: TriggerMeta,
    /// Lane key for queue serialization. Defaults to the conversation id so
    /// turns in the same conversation run strictly in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub enabled_skill_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_agent_id: Option<String>,
    #[serde(default)]
    pub tool_rounds_used_single_start: u32,
    #[serde(default)]
    pub tool_rounds_used_supervisor_start: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_inherit_disabled: Option<bool>,
    #[serde(default)]
    pub deliver: DeliverTarget,
    /// Per-browser OAuth session for pointer-server HTTP runs (not serialized).
    #[serde(skip, default)]
    pub web_session_auth: Option<WebSessionAuth>,
}

/// Immediate response from [`crate::dispatcher::RunDispatcher::dispatch`].
/// Follows the openclaw ack shape: a run is accepted (or reused via
/// idempotency) and the caller subscribes to the event bus for progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHandle {
    pub run_id: String,
    pub status: RunAcceptStatus,
    /// Set when `idempotency_key` matched an existing run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAcceptStatus {
    /// Run was newly accepted and queued.
    Accepted,
    /// Run was already in progress with the same idempotency key.
    Reused,
}

/// Terminal outcome for [`crate::dispatcher::RunDispatcher::wait`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunOutcome {
    Finished { run_id: String, conversation_id: String },
    Failed { run_id: String, conversation_id: String, error: String },
    Cancelled { run_id: String, conversation_id: String },
}
