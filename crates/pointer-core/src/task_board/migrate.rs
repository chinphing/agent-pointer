//! Load persisted task board JSON as v3 [`BoardDocument`] only.

use super::model::{BoardDocument, BOARD_VERSION};
use serde_json::Value;

pub fn normalize_stored_value(store_key: &str, raw: Value) -> BoardDocument {
    if let Ok(doc) = serde_json::from_value::<BoardDocument>(raw.clone()) {
        if doc.version == BOARD_VERSION && !doc.task_id.is_empty() {
            return doc;
        }
        if doc.version != BOARD_VERSION {
            log::warn!(
                "task_board: ignoring stored document version={} (expected {BOARD_VERSION}) store_key={store_key}",
                doc.version,
            );
        }
    }
    BoardDocument::empty_for_store_key(store_key)
}
