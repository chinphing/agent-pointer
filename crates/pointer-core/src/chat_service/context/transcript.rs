//! Conversation transcript buffers (main history vs sub-agent local history).

use crate::models::ChatMessage;

use super::super::sub_message::SubMessageLinkage;

/// Mutable message history for the current agent scope.
pub struct TranscriptRefs<'a> {
    pub history: &'a mut Vec<ChatMessage>,
}

/// How assistant/tool rows are persisted for this tool-pass scope.
#[derive(Debug, Clone)]
pub enum TranscriptPersist {
    /// Lead single-agent / supervisor: main transcript flush + upsert.
    Main,
    /// Sub-agent: per-row upsert with parent linkage (not a full-transcript flush).
    SubLinked(SubMessageLinkage),
}

impl TranscriptPersist {
    pub fn persist_transcript(&self) -> bool {
        true
    }

    pub fn flush_tool_pass_history(&self) -> bool {
        matches!(self, TranscriptPersist::Main)
    }
}
