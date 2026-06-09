//! Session search tool — queries the canonical `ConversationStore`.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

pub use crate::conversation_store::ConversationStore;

/// Back-compat alias used by `AppState`.
pub type SessionIndex = ConversationStore;

pub fn open_default() -> Result<Arc<ConversationStore>> {
    crate::conversation_store::global_store()
}

pub fn open(path: PathBuf) -> Result<Arc<ConversationStore>> {
    Ok(Arc::new(ConversationStore::open(path)?))
}
