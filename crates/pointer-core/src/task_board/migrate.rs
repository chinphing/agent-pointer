//! v1 flat array → v2 [`BoardDocument`].

use super::model::{BoardDocument, BoardItem, BOARD_VERSION};
use serde_json::Value;

pub fn normalize_stored_value(store_key: &str, raw: Value) -> BoardDocument {
    if let Ok(doc) = serde_json::from_value::<BoardDocument>(raw.clone()) {
        if doc.version >= BOARD_VERSION && !doc.task_id.is_empty() {
            return doc;
        }
    }
    if let Some(arr) = raw.as_array() {
        let mut doc = BoardDocument::empty_for_store_key(store_key);
        doc.version = BOARD_VERSION;
        for v in arr {
            if let Some(item) = BoardItem::from_value(v) {
                doc.board.push(item);
            }
        }
        return doc;
    }
    BoardDocument::empty_for_store_key(store_key)
}
