//! Session prompt assembly snapshot for cache-reusing compression calls.
//!
//! The default compression path builds a summarizer-only prompt (own system
//! prompt, history re-rendered as plain text, no tools), which shares **no**
//! prefix with the session request — so provider prefix caching never hits.
//!
//! Codex instead runs compaction on the live prompt: the compaction prompt is
//! appended as a normal user turn while the same developer instructions, the
//! same tool definitions, and the same history are reused, so the whole prefix
//! is served from cache (`codex-rs/core/src/compact.rs`).
//!
//! Pointer keeps that shape by snapshotting the **inputs** of the session
//! round's request assembly. Rebuilding the wire from those inputs is
//! byte-deterministic ([`crate::models::make_openai_messages_with_inject`] emits
//! no internal ids, no timestamps, and no map-iteration order), so a compression
//! call can send `[session system][session tools][history[..drop_end)]` plus a
//! trailing instruction and hit the cache for everything but the instruction.

use super::types::{CompressionScope, CompressionUiContext};
use crate::message_context::LlmHistoryScope;
use crate::models::{ChatMessage, ModelSettings, SystemPromptSections};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Inputs the session round used to build its HTTP request.
#[derive(Clone)]
pub(crate) struct SessionPromptSnapshot {
    /// `cacheable` + `dynamic` slices exactly as that round rendered them.
    pub system: SystemPromptSections,
    /// Flat tool schemas sent as the request `tools[]` field.
    pub native_tools: Vec<Value>,
    /// Transcript filter used by that round (lead vs sub-agent loop).
    pub history_scope: LlmHistoryScope,
    /// Provider caches are scoped to provider + model, so a mismatch must not reuse.
    pub provider_id: String,
    pub model: String,
    /// Last base-history row that round sent (cheap staleness probe).
    pub last_message_id: String,
}

impl SessionPromptSnapshot {
    pub(crate) fn matches_settings(&self, settings: &ModelSettings) -> bool {
        self.provider_id == settings.active_provider_id && self.model == settings.model
    }

    /// The snapshotted round must still sit inside the history we send as prefix.
    /// Guards against a stale snapshot (history rewritten by an earlier
    /// compression, conversation reloaded, ...).
    pub(crate) fn covers_history(&self, history: &[ChatMessage]) -> bool {
        if self.last_message_id.is_empty() {
            return true;
        }
        history.iter().any(|m| m.id == self.last_message_id)
    }
}

/// `(conversation_id, agent_instance_id)` — lead rounds use an empty instance id.
type SnapshotKey = (String, String);

fn snapshots() -> &'static Mutex<HashMap<SnapshotKey, SessionPromptSnapshot>> {
    static SNAPSHOTS: OnceLock<Mutex<HashMap<SnapshotKey, SessionPromptSnapshot>>> =
        OnceLock::new();
    SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn snapshot_key(conversation_id: &str, agent_instance_id: Option<&str>) -> SnapshotKey {
    (
        conversation_id.trim().to_string(),
        agent_instance_id.unwrap_or("").trim().to_string(),
    )
}

/// Keep the registry bounded: one entry per live (conversation, agent instance)
/// holds a tool-schema clone, so long-lived sessions with many sub-agents must
/// not accumulate them forever. Compression is opportunistic — losing a
/// snapshot only means falling back to the legacy summarizer prompt.
const MAX_SNAPSHOTS: usize = 32;

/// Record the assembly inputs of the round that is about to be sent.
///
/// Overwrites any earlier snapshot for the same scope: only the newest round can
/// still be a prefix of what we send next.
pub(crate) fn remember_session_prompt(
    conversation_id: &str,
    agent_instance_id: Option<&str>,
    snapshot: SessionPromptSnapshot,
) {
    let id = conversation_id.trim();
    if id.is_empty() {
        return;
    }
    let key = snapshot_key(id, agent_instance_id);
    log::debug!(
        "context_compress: remember session prompt conversation_id={} agent_instance_id={} cacheable_slices={} dynamic_slices={} tools={} scope={:?}",
        key.0,
        key.1,
        snapshot.system.cacheable.len(),
        snapshot.system.dynamic.len(),
        snapshot.native_tools.len(),
        snapshot.history_scope
    );
    let mut map = snapshots().lock();
    if !map.contains_key(&key) && map.len() >= MAX_SNAPSHOTS {
        log::info!(
            "context_compress: session prompt snapshots full ({} entries), dropping all",
            map.len()
        );
        map.clear();
    }
    map.insert(key, snapshot);
}

pub(crate) fn session_prompt_for(
    conversation_id: &str,
    agent_instance_id: Option<&str>,
) -> Option<SessionPromptSnapshot> {
    let key = snapshot_key(conversation_id, agent_instance_id);
    snapshots().lock().get(&key).cloned()
}

/// Sub-agent compression runs on its own instance id; lead compression has none.
pub(crate) fn scope_key_from_ui(ui: &CompressionUiContext) -> Option<&str> {
    match ui.scope {
        CompressionScope::Main => None,
        CompressionScope::SubAgent => ui
            .agent_scope
            .as_ref()
            .map(|scope| scope.agent_instance_id.as_str()),
    }
}
