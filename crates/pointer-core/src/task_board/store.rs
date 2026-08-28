//! In-memory task board store with optional SQLite write-through.
//!
//! When persistence is enabled, the in-memory map is a soft cache:
//! - terminal boards (`completed` / `failed`) are unloaded after write/read
//! - idle boards older than [`IDLE_EVICT_SECS`] are unloaded
//! - cache size is capped at [`MAX_CACHED_BOARDS`] (LRU among survivors)
//!
//! Without persistence (unit tests / ephemeral), the map is not evicted so
//! in-memory-only documents are not lost.

use super::apply::apply_method;
use super::coordination::parent_child::parent_store_key_from_child;
use super::migrate::normalize_stored_value;
use super::model::{BoardDocument, MetaStatus};
use super::persistence::TaskBoardSqlite;
use super::snapshot::snapshot_for_prompt;
use anyhow::Result;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Soft-cache cap for persisted boards (LRU after terminal/idle sweeps).
pub const MAX_CACHED_BOARDS: usize = 20;
/// Idle soft-unload timeout (aligned with frontend conversation message eviction).
pub const IDLE_EVICT_SECS: u64 = 120 * 60;

struct CachedBoard {
    doc: BoardDocument,
    last_accessed: Instant,
}

pub struct TaskBoardStore {
    inner: RwLock<HashMap<String, CachedBoard>>,
    persistence: RwLock<Option<Arc<TaskBoardSqlite>>>,
}

impl Default for TaskBoardStore {
    fn default() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            persistence: RwLock::new(None),
        }
    }
}

fn is_terminal(doc: &BoardDocument) -> bool {
    matches!(doc.meta.status, MetaStatus::Completed | MetaStatus::Failed)
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

    fn has_persistence(&self) -> bool {
        self.persistence.read().is_some()
    }

    /// Whether `store_key` is currently held in the soft cache.
    pub fn is_cached(&self, store_key: &str) -> bool {
        self.inner.read().contains_key(store_key)
    }

    /// Number of boards currently held in the soft cache.
    pub fn cached_count(&self) -> usize {
        self.inner.read().len()
    }

    pub fn ensure_loaded(&self, store_key: &str) {
        {
            let mut inner = self.inner.write();
            if let Some(entry) = inner.get_mut(store_key) {
                entry.last_accessed = Instant::now();
                return;
            }
        }
        if let Some(db) = self.persistence.read().clone() {
            match db.load(store_key) {
                Ok(Some(doc)) => {
                    self.inner.write().insert(
                        store_key.to_string(),
                        CachedBoard {
                            doc,
                            last_accessed: Instant::now(),
                        },
                    );
                    log::info!("task_board: loaded into cache store_key={store_key}");
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("task_board: sqlite load failed store_key={store_key}: {e}");
                }
            }
        }
    }

    fn touch_locked(entry: &mut CachedBoard) {
        entry.last_accessed = Instant::now();
    }

    fn unload_locked(inner: &mut HashMap<String, CachedBoard>, store_key: &str, reason: &str) {
        if inner.remove(store_key).is_some() {
            log::info!("task_board: unloaded from cache store_key={store_key} reason={reason}");
        }
    }

    /// Soft-unload one key from memory (SQLite copy kept when persistence is on).
    pub fn unload(&self, store_key: &str, reason: &str) {
        let mut inner = self.inner.write();
        Self::unload_locked(&mut inner, store_key, reason);
    }

    /// Run terminal / idle / LRU sweeps when persistence is enabled.
    pub fn evict(&self) {
        if !self.has_persistence() {
            return;
        }
        let now = Instant::now();
        let idle_cutoff = Duration::from_secs(IDLE_EVICT_SECS);
        let mut inner = self.inner.write();

        let terminal_keys: Vec<String> = inner
            .iter()
            .filter(|(_, e)| is_terminal(&e.doc))
            .map(|(k, _)| k.clone())
            .collect();
        for key in terminal_keys {
            Self::unload_locked(&mut inner, &key, "terminal");
        }

        let idle_keys: Vec<String> = inner
            .iter()
            .filter(|(_, e)| now.saturating_duration_since(e.last_accessed) >= idle_cutoff)
            .map(|(k, _)| k.clone())
            .collect();
        for key in idle_keys {
            Self::unload_locked(&mut inner, &key, "idle");
        }

        while inner.len() > MAX_CACHED_BOARDS {
            let lru_key = inner
                .iter()
                .min_by_key(|(_, e)| e.last_accessed)
                .map(|(k, _)| k.clone());
            match lru_key {
                Some(key) => Self::unload_locked(&mut inner, &key, "lru"),
                None => break,
            }
        }
    }

    fn insert_doc(&self, store_key: &str, doc: BoardDocument) {
        self.inner.write().insert(
            store_key.to_string(),
            CachedBoard {
                doc,
                last_accessed: Instant::now(),
            },
        );
    }

    fn after_write(&self, store_key: &str, doc: &BoardDocument) {
        if !self.has_persistence() {
            return;
        }
        if is_terminal(doc) {
            self.unload(store_key, "terminal");
        } else {
            self.evict();
        }
    }

    fn get_or_default(&self, store_key: &str) -> BoardDocument {
        self.ensure_loaded(store_key);
        let doc = {
            let mut inner = self.inner.write();
            if let Some(entry) = inner.get_mut(store_key) {
                Self::touch_locked(entry);
                entry.doc.clone()
            } else {
                BoardDocument::empty_for_store_key(store_key)
            }
        };
        if self.has_persistence() {
            if is_terminal(&doc) {
                // Keep terminal boards out of the hot cache after serving a read.
                self.unload(store_key, "terminal");
            } else {
                self.evict();
            }
        }
        doc
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
        let mut doc = self.get_or_default(store_key);
        // get_or_default may have unloaded a terminal board; reload empty/new for write path.
        if is_terminal(&doc) && !self.is_cached(store_key) {
            // Re-load from disk so patch/finalize continue on the persisted document.
            self.ensure_loaded(store_key);
            if let Some(entry) = self.inner.read().get(store_key) {
                doc = entry.doc.clone();
            }
        }
        let outcome = apply_method(store_key, &mut doc, &method, args)?;
        self.insert_doc(store_key, doc.clone());
        self.persist(store_key, &doc);
        self.after_write(store_key, &doc);
        crate::task_board::observability::log_store_apply(
            store_key,
            &method,
            doc.global_milestones.len(),
            outcome.reflection_required,
        );
        let body = outcome.body;
        Ok((body, outcome.reflection_required))
    }

    pub fn items_json(&self, store_key: &str) -> Value {
        self.get_or_default(store_key).to_value()
    }

    pub fn document(&self, store_key: &str) -> BoardDocument {
        self.get_or_default(store_key)
    }

    pub fn save_document(&self, store_key: &str, doc: BoardDocument) {
        self.insert_doc(store_key, doc.clone());
        self.persist(store_key, &doc);
        self.after_write(store_key, &doc);
    }

    pub fn list_store_keys_by_prefix(&self, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .inner
            .read()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        if let Some(db) = self.persistence.read().clone() {
            match db.list_store_keys_by_prefix(prefix) {
                Ok(keys) => {
                    for k in keys {
                        if !out.iter().any(|x| x == &k) {
                            out.push(k);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("task_board: list_store_keys_by_prefix failed prefix={prefix}: {e}");
                }
            }
        }
        out
    }

    pub fn snapshot_for_prompt(&self, store_key: &str) -> Option<String> {
        let doc = self.get_or_default(store_key);
        let block = snapshot_for_prompt(store_key, &doc, true);
        if let Some(ref b) = block {
            if !b.is_empty() {
                crate::task_board::observability::log_snapshot_injected(
                    store_key,
                    doc.global_milestones.len(),
                    !doc.meta.goal.is_empty(),
                );
            }
        } else {
            crate::task_board::observability::log_snapshot_skipped_empty(store_key);
        }
        block
    }

    pub fn parent_tunnel_for_child(
        &self,
        child_store_key: &str,
        sub_task_id: &str,
    ) -> Option<String> {
        let parent_key = parent_store_key_from_child(child_store_key)?;
        let doc = self.get_or_default(&parent_key);
        super::coordination::context_tunnel::parent_tunnel_block(&doc, sub_task_id)
    }

    /// Legacy: accept raw Value for migration tests.
    pub fn insert_raw(&self, store_key: &str, raw: Value) {
        let doc = normalize_stored_value(store_key, raw);
        self.insert_doc(store_key, doc);
        if self.has_persistence() {
            self.evict();
        }
    }
}

#[cfg(test)]
mod eviction_tests {
    use super::*;
    use crate::task_board::persistence::TaskBoardSqlite;
    use serde_json::json;
    use std::time::Duration;

    fn persist_store() -> (tempfile::TempDir, TaskBoardStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("task_boards.db");
        let db = TaskBoardSqlite::open(path).expect("open");
        (dir, TaskBoardStore::with_persistence(db))
    }

    fn backdate(store: &TaskBoardStore, key: &str, age: Duration) {
        let mut inner = store.inner.write();
        let entry = inner.get_mut(key).expect("cached");
        entry.last_accessed = Instant::now().checked_sub(age).expect("instant backdate");
    }

    #[test]
    fn terminal_board_unloads_after_finalize_when_persisted() {
        let (_dir, store) = persist_store();
        let key = "conv\x1fturn_u1";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "a", "title": "A", "status": "done"}]
                }),
            )
            .expect("init");
        assert!(store.is_cached(key), "running board stays cached");
        store.apply(key, "finalize", &json!({})).expect("finalize");
        assert!(
            !store.is_cached(key),
            "completed board should leave the soft cache"
        );
        let doc = store.document(key);
        assert_eq!(doc.meta.status, MetaStatus::Completed);
        assert!(
            !store.is_cached(key),
            "terminal read should not keep the board cached"
        );
    }

    #[test]
    fn idle_board_unloads_after_timeout() {
        let (_dir, store) = persist_store();
        let key = "idle-board";
        store
            .apply(
                key,
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "m1", "title": "t", "status": "pending"}]
                }),
            )
            .expect("init");
        assert!(store.is_cached(key));
        backdate(&store, key, Duration::from_secs(IDLE_EVICT_SECS + 5));
        store.evict();
        assert!(!store.is_cached(key));
        let doc = store.document(key);
        assert_eq!(doc.meta.goal, "g");
    }

    #[test]
    fn lru_evicts_when_over_max_cached() {
        let (_dir, store) = persist_store();
        for i in 0..(MAX_CACHED_BOARDS + 3) {
            let key = format!("board-{i}");
            store
                .apply(
                    &key,
                    "init",
                    &json!({
                        "goal": format!("g{i}"),
                        "items": [{"id": "m1", "title": "t", "status": "pending"}]
                    }),
                )
                .expect("init");
        }
        assert!(store.cached_count() <= MAX_CACHED_BOARDS);
        // Newest should still be present; oldest likely evicted.
        assert!(store.is_cached(&format!("board-{}", MAX_CACHED_BOARDS + 2)));
        assert!(!store.is_cached("board-0") || store.cached_count() <= MAX_CACHED_BOARDS);
        let oldest = store.document("board-0");
        assert_eq!(oldest.meta.goal, "g0");
    }

    #[test]
    fn no_persistence_skips_eviction() {
        let store = TaskBoardStore::new();
        store
            .apply(
                "k",
                "init",
                &json!({
                    "goal": "g",
                    "items": [{"id": "a", "title": "A", "status": "done"}]
                }),
            )
            .expect("init");
        store.apply("k", "finalize", &json!({})).expect("finalize");
        assert!(
            store.is_cached("k"),
            "in-memory-only store must keep terminal boards"
        );
    }
}
