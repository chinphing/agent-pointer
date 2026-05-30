//! Auto-finalize boards when all rows are terminal but `meta.status` is still `running`.

use super::model::MetaStatus;
use super::state_machine::count_incomplete;
use super::store::TaskBoardStore;
use serde_json::json;

/// If every row is `done` or `cancelled` and meta is still `running`, apply `finalize`.
pub fn maybe_auto_finalize_if_complete(store: &TaskBoardStore, store_key: &str) -> bool {
    let doc = store.document(store_key);
    if doc.meta.goal.trim().is_empty() && doc.board_is_empty() {
        return false;
    }
    if doc.meta.status != MetaStatus::Running {
        return false;
    }
    if count_incomplete(&doc) > 0 {
        return false;
    }
    match store.apply(store_key, "finalize", &json!({})) {
        Ok(_) => {
            log::info!("task_board: auto-finalized store_key={store_key}");
            true
        }
        Err(e) => {
            log::warn!("task_board: auto-finalize failed store_key={store_key}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_board::model::MetaStatus;

    #[test]
    fn auto_finalize_when_all_rows_done() {
        let store = TaskBoardStore::new();
        let key = "conv-1";
        store
            .apply(
                key,
                "init",
                &serde_json::json!({
                    "goal": "ship",
                    "items": [
                        { "id": "m1", "title": "a", "status": "done" },
                        { "id": "m2", "title": "b", "status": "cancelled" }
                    ]
                }),
            )
            .expect("init");
        assert!(maybe_auto_finalize_if_complete(&store, key));
        assert_eq!(
            store.document(key).meta.status,
            MetaStatus::Completed
        );
    }

    #[test]
    fn auto_finalize_skips_when_rows_incomplete() {
        let store = TaskBoardStore::new();
        let key = "conv-2";
        store
            .apply(
                key,
                "init",
                &serde_json::json!({
                    "goal": "ship",
                    "items": [{ "id": "m1", "title": "a", "status": "pending" }]
                }),
            )
            .expect("init");
        assert!(!maybe_auto_finalize_if_complete(&store, key));
        assert_eq!(store.document(key).meta.status, MetaStatus::Running);
    }
}
