//! In-memory task board store with optional SQLite write-through.

use super::apply::{apply_method, apply_sync_finding_to_doc};
use super::args::finding_from_args;
use super::coordination::parent_child::parent_store_key_from_child;
use super::migrate::normalize_stored_value;
use super::model::BoardDocument;
use super::persistence::TaskBoardSqlite;
use super::snapshot::snapshot_for_prompt;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
pub struct TaskBoardStore {
    inner: RwLock<HashMap<String, BoardDocument>>,
    persistence: RwLock<Option<Arc<TaskBoardSqlite>>>,
}

impl TaskBoardStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_persistence(db: Arc<TaskBoardSqlite>) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            persistence: RwLock::new(Some(db)),
        }
    }

    pub fn set_persistence(&self, db: Option<Arc<TaskBoardSqlite>>) {
        *self.persistence.write() = db;
    }

    pub fn ensure_loaded(&self, store_key: &str) {
        if self.inner.read().contains_key(store_key) {
            return;
        }
        if let Some(db) = self.persistence.read().clone() {
            if let Ok(Some(doc)) = db.load(store_key) {
                self.inner.write().insert(store_key.to_string(), doc);
            }
        }
    }

    fn get_or_default(&self, store_key: &str) -> BoardDocument {
        self.ensure_loaded(store_key);
        self.inner
            .read()
            .get(store_key)
            .cloned()
            .unwrap_or_else(|| BoardDocument::empty_for_store_key(store_key))
    }

    fn persist(&self, store_key: &str, doc: &BoardDocument) {
        if let Some(db) = self.persistence.read().clone() {
            if let Err(e) = db.save(store_key, doc) {
                log::warn!("task_board: sqlite save failed store_key={store_key}: {e}");
            }
        }
    }

    pub fn apply(&self, store_key: &str, method: &str, args: &Value) -> Result<(Value, bool)> {
        let method = method.trim().to_ascii_lowercase();
        if method == "sync_finding" {
            return self.apply_sync_finding_route(store_key, args);
        }
        let mut doc = self.get_or_default(store_key);
        let outcome = apply_method(store_key, &mut doc, &method, args)?;
        self.inner.write().insert(store_key.to_string(), doc.clone());
        self.persist(store_key, &doc);
        crate::task_board::observability::log_store_apply(
            store_key,
            &method,
            doc.board.len(),
            outcome.reflection_required,
        );
        let body = outcome.body;
        Ok((body, outcome.reflection_required))
    }

    fn apply_sync_finding_route(&self, child_store_key: &str, args: &Value) -> Result<(Value, bool)> {
        let finding = finding_from_args(args)
            .ok_or_else(|| anyhow!("task_board: sync_finding requires finding"))?;
        let parent_key = parent_store_key_from_child(child_store_key)
            .ok_or_else(|| anyhow!("task_board: sync_finding only from child board"))?;
        let mut parent = self.get_or_default(&parent_key);
        let body = apply_sync_finding_to_doc(&mut parent, &finding)?;
        self.inner.write().insert(parent_key.clone(), parent.clone());
        self.persist(&parent_key, &parent);
        Ok((body, false))
    }

    pub fn items_json(&self, store_key: &str) -> Value {
        self.get_or_default(store_key).to_value()
    }

    pub fn document(&self, store_key: &str) -> BoardDocument {
        self.get_or_default(store_key)
    }

    pub fn save_document(&self, store_key: &str, doc: BoardDocument) {
        self.inner.write().insert(store_key.to_string(), doc.clone());
        self.persist(store_key, &doc);
    }

    pub fn snapshot_for_prompt(&self, store_key: &str) -> Option<String> {
        let doc = self.get_or_default(store_key);
        let block = snapshot_for_prompt(store_key, &doc, true);
        if let Some(ref b) = block {
            if !b.is_empty() {
                crate::task_board::observability::log_snapshot_injected(
                    store_key,
                    doc.board.len(),
                    !doc.meta.goal.is_empty(),
                );
            }
        } else {
            crate::task_board::observability::log_snapshot_skipped_empty(store_key);
        }
        block
    }

    pub fn parent_tunnel_for_child(&self, child_store_key: &str, sub_task_id: &str) -> Option<String> {
        let parent_key = parent_store_key_from_child(child_store_key)?;
        let doc = self.get_or_default(&parent_key);
        super::coordination::context_tunnel::parent_tunnel_block(&doc, sub_task_id)
    }

    /// Legacy: accept raw Value for migration tests.
    pub fn insert_raw(&self, store_key: &str, raw: Value) {
        let doc = normalize_stored_value(store_key, raw);
        self.inner.write().insert(store_key.to_string(), doc);
    }
}
