use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::ConversationTranscriptSession;

pub struct ConversationTranscriptRegistry {
    active: Mutex<HashMap<String, Arc<Mutex<ConversationTranscriptSession>>>>,
}

impl ConversationTranscriptRegistry {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        conversation_id: &str,
        session: Arc<Mutex<ConversationTranscriptSession>>,
    ) {
        self.active
            .lock()
            .insert(conversation_id.to_string(), session);
    }

    pub fn unregister(&self, conversation_id: &str) {
        self.active.lock().remove(conversation_id);
    }

    pub fn get(
        &self,
        conversation_id: &str,
    ) -> Option<Arc<Mutex<ConversationTranscriptSession>>> {
        self.active.lock().get(conversation_id).cloned()
    }
}
