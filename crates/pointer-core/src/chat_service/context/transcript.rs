//! Conversation transcript buffers (main history vs sub-agent local history).

use crate::models::ChatMessage;

/// Mutable message history for the current agent scope.
pub struct TranscriptRefs<'a> {
    pub history: &'a mut Vec<ChatMessage>,
}

/// Whether assistant/tool rows should be persisted to the main conversation DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptPersist {
    /// Lead single-agent / supervisor: write to main session DB.
    Main,
    /// Sub-agent: in-memory `local_history` only.
    LocalOnly,
}

impl TranscriptPersist {
    pub fn persist_transcript(self) -> bool {
        matches!(self, TranscriptPersist::Main)
    }
}
