//! In-memory work item store with optional SQLite write-through.

use super::model::{
    draft_from_value, merge_result_summary, now_ms, parse_work_item_seq_ref,
    payload_with_target_key, target_key_from_payload, work_item_id, BatchStats, SeedOutcome,
    StoreStats, WorkItem, WorkItemDraft, WorkItemStatus, MAX_INLINE_SEED,
};
use super::persistence::WorkItemSqlite;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub const WORK_ITEM_INJECT_IN_PROGRESS: usize = 1;
pub const WORK_ITEM_INJECT_RECENT_DONE: usize = 3;
pub const WORK_ITEM_INJECT_NEXT_READY: usize = 2;

#[derive(Debug, Clone)]
pub struct WorkItemDelta {
    pub id: String,
    pub status: WorkItemStatus,
    pub result_summary: Option<String>,
    pub result_ref: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkItemClaim {
    pub target_key: String,
    pub title: String,
    pub payload: Value,
    pub status: WorkItemStatus,
}

pub struct WorkItemStore {
    inner: RwLock<HashMap<String, HashMap<String, WorkItem>>>,
    persistence: RwLock<Option<Arc<WorkItemSqlite>>>,
}

impl WorkItemStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_persistence(db: Arc<WorkItemSqlite>) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            persistence: RwLock::new(Some(db)),
        }
    }

    pub fn set_persistence(&self, db: Option<Arc<WorkItemSqlite>>) {
        *self.persistence.write() = db;
    }

    fn ensure_loaded(&self, store_id: &str) {
        if self.inner.read().contains_key(store_id) {
            return;
        }
        if let Some(db) = self.persistence.read().clone() {
            match db.load_store(store_id) {
                Ok(items) => {
                    let mut map = HashMap::new();
                    for item in items {
                        map.insert(item.id.clone(), item);
                    }
                    self.inner.write().insert(store_id.to_string(), map);
                }
                Err(e) => {
                    log::warn!("work_items: load failed store_id={store_id}: {e}");
                }
            }
        }
    }

    fn store_map(&self, store_id: &str) -> HashMap<String, WorkItem> {
        self.ensure_loaded(store_id);
        self.inner
            .read()
            .get(store_id)
            .cloned()
            .unwrap_or_default()
    }

    fn persist_item(&self, item: &WorkItem) {
        if let Some(db) = self.persistence.read().clone() {
            if let Err(e) = db.upsert(item) {
                log::warn!("work_items: sqlite upsert failed id={}: {e}", item.id);
            }
        }
    }

    fn insert_item(&self, store_id: &str, item: WorkItem) {
        self.inner
            .write()
            .entry(store_id.to_string())
            .or_default()
            .insert(item.id.clone(), item.clone());
        self.persist_item(&item);
    }

    fn next_seq(&self, store_id: &str) -> i64 {
        let map = self.store_map(store_id);
        map.values().map(|i| i.seq).max().unwrap_or(0) + 1
    }

    pub fn replace_store(&self, store_id: &str) -> Result<()> {
        self.inner.write().remove(store_id);
        if let Some(db) = self.persistence.read().clone() {
            db.delete_store(store_id)?;
        }
        Ok(())
    }

    pub fn seed(
        &self,
        store_id: &str,
        drafts: Vec<WorkItemDraft>,
    ) -> Result<SeedOutcome> {
        if drafts.len() > MAX_INLINE_SEED {
            return Err(anyhow!(
                "work_items: inline seed exceeds MAX_INLINE_SEED ({MAX_INLINE_SEED})"
            ));
        }
        let now = now_ms();
        let mut seeded = 0u32;
        for draft in drafts {
            let payload = payload_with_target_key(draft.payload, draft.target_key.clone());
            let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
            if let Some(ref key) = draft.target_key {
                if self.target_key_exists(store_id, key)? {
                    return Err(anyhow!("duplicate_target_key: {key}"));
                }
            }
            let seq = self.next_seq(store_id);
            let id = work_item_id(store_id, seq);
            let item = WorkItem {
                id,
                store_id: store_id.to_string(),
                seq,
                status: WorkItemStatus::Pending,
                title: draft.title,
                payload_json,
                retry_count: 0,
                max_retries: 2,
                result_ref: None,
                result_json: None,
                error_message: None,
                created_at_ms: now,
                updated_at_ms: now,
                started_at_ms: None,
                finished_at_ms: None,
            };
            self.insert_item(store_id, item);
            seeded += 1;
        }
        log::info!("work_items: seeded store_id={store_id} count={seeded}");
        Ok(SeedOutcome { seeded })
    }

    pub fn seed_from_values(&self, store_id: &str, values: &[Value]) -> Result<SeedOutcome> {
        let drafts: Vec<WorkItemDraft> = values.iter().filter_map(draft_from_value).collect();
        if drafts.is_empty() && !values.is_empty() {
            return Err(anyhow!("work_items: no valid work_items entries in seed"));
        }
        self.seed(store_id, drafts)
    }

    pub fn apply_delta(&self, store_id: &str, delta: WorkItemDelta) -> Result<WorkItem> {
        let mut map = self.store_map(store_id);
        let lookup = delta.id.clone();
        let resolved_key = Self::resolve_item_key(&map, &lookup)
            .ok_or_else(|| anyhow!("work_item_not_found: {lookup}"))?;
        let item = map
            .get_mut(&resolved_key)
            .ok_or_else(|| anyhow!("work_item_not_found: {lookup}"))?;
        let now = now_ms();
        item.status = delta.status;
        item.updated_at_ms = now;
        if delta.status == WorkItemStatus::InProgress && item.started_at_ms.is_none() {
            item.started_at_ms = Some(now);
        }
        if delta.status.is_terminal() {
            item.finished_at_ms = Some(now);
        }
        if let Some(ref summary) = delta.result_summary {
            item.result_json = Some(merge_result_summary(
                item.result_json.as_deref(),
                summary,
            ));
        }
        if let Some(ref r) = delta.result_ref {
            item.result_ref = Some(r.clone());
        }
        if let Some(ref e) = delta.error_message {
            item.error_message = Some(e.clone());
        }
        let canonical_id = work_item_id(store_id, item.seq);
        if item.id != canonical_id {
            item.id = canonical_id;
        }
        let out = item.clone();
        let mut store = self.inner.write();
        let entry = store.entry(store_id.to_string()).or_default();
        if resolved_key != out.id {
            entry.remove(&resolved_key);
        }
        entry.insert(out.id.clone(), out.clone());
        self.persist_item(&out);
        Ok(out)
    }

    fn resolve_item_key(map: &HashMap<String, WorkItem>, delta_id: &str) -> Option<String> {
        if map.contains_key(delta_id) {
            return Some(delta_id.to_string());
        }
        let seq = parse_work_item_seq_ref(delta_id)?;
        map.iter()
            .find(|(_, item)| item.seq == seq)
            .map(|(k, _)| k.clone())
    }

    pub fn claim(
        &self,
        store_id: &str,
        claim: WorkItemClaim,
        quota: u32,
    ) -> Result<WorkItem> {
        let stats = self.store_stats(store_id);
        let active = stats.done + stats.failed + stats.in_progress;
        if active >= quota {
            return Err(anyhow!("quota_exhausted"));
        }
        let key = claim.target_key.trim();
        if key.is_empty() {
            return Err(anyhow!("work_items: claim requires target_key"));
        }
        if self.target_key_exists(store_id, key)? {
            return Err(anyhow!("duplicate_target_key: {key}"));
        }
        let now = now_ms();
        let seq = self.next_seq(store_id);
        let id = work_item_id(store_id, seq);
        let payload = payload_with_target_key(claim.payload, Some(key.to_string()));
        let item = WorkItem {
            id,
            store_id: store_id.to_string(),
            seq,
            status: claim.status,
            title: claim.title,
            payload_json: serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into()),
            retry_count: 0,
            max_retries: 2,
            result_ref: None,
            result_json: None,
            error_message: None,
            created_at_ms: now,
            updated_at_ms: now,
            started_at_ms: Some(now),
            finished_at_ms: None,
        };
        self.insert_item(store_id, item.clone());
        log::info!(
            "work_items: claim store_id={store_id} id={} target_key={key}",
            item.id
        );
        Ok(item)
    }

    pub fn target_key_exists(&self, store_id: &str, target_key: &str) -> Result<bool> {
        self.ensure_loaded(store_id);
        if let Some(map) = self.inner.read().get(store_id) {
            for item in map.values() {
                if target_key_from_payload(&item.payload_json).as_deref() == Some(target_key) {
                    return Ok(true);
                }
            }
        }
        if let Some(db) = self.persistence.read().clone() {
            return db.target_key_exists(store_id, target_key);
        }
        Ok(false)
    }

    pub fn store_stats(&self, store_id: &str) -> BatchStats {
        let map = self.store_map(store_id);
        let mut stats = BatchStats::default();
        for item in map.values() {
            stats.total += 1;
            match item.status {
                WorkItemStatus::Done => stats.done += 1,
                WorkItemStatus::Failed => stats.failed += 1,
                WorkItemStatus::InProgress => stats.in_progress += 1,
                WorkItemStatus::Pending | WorkItemStatus::Ready => stats.pending += 1,
                WorkItemStatus::Cancelled => {}
            }
        }
        stats
    }

    pub fn aggregate_stats(&self, store_id: &str) -> StoreStats {
        let map = self.store_map(store_id);
        let mut stats = StoreStats::default();
        for item in map.values() {
            stats.total += 1;
            match item.status {
                WorkItemStatus::Done => stats.done += 1,
                WorkItemStatus::Failed => stats.failed += 1,
                WorkItemStatus::InProgress => stats.in_progress += 1,
                _ => {}
            }
        }
        stats
    }

    pub fn store_has_done(&self, store_id: &str) -> bool {
        self.store_stats(store_id).done > 0
    }

    pub fn inject_window(&self, store_id: &str) -> Vec<WorkItem> {
        let map = self.store_map(store_id);
        let mut items: Vec<WorkItem> = map.values().cloned().collect();
        items.sort_by_key(|i| i.seq);

        let mut out = Vec::new();
        for item in items
            .iter()
            .filter(|i| i.status == WorkItemStatus::InProgress)
            .take(WORK_ITEM_INJECT_IN_PROGRESS)
        {
            out.push(item.clone());
        }
        let mut recent_done: Vec<WorkItem> = items
            .iter()
            .filter(|i| i.status == WorkItemStatus::Done)
            .cloned()
            .collect();
        recent_done.sort_by_key(|i| i.finished_at_ms.unwrap_or(i.updated_at_ms));
        for item in recent_done
            .into_iter()
            .rev()
            .take(WORK_ITEM_INJECT_RECENT_DONE)
        {
            if !out.iter().any(|x| x.id == item.id) {
                out.push(item);
            }
        }
        for item in items
            .iter()
            .filter(|i| matches!(i.status, WorkItemStatus::Pending | WorkItemStatus::Ready))
            .take(WORK_ITEM_INJECT_NEXT_READY)
        {
            if !out.iter().any(|x| x.id == item.id) {
                out.push(item.clone());
            }
        }
        out
    }

    pub fn store_total(&self, store_id: &str) -> u32 {
        self.store_stats(store_id).total
    }

    pub fn count_store(&self, store_id: &str) -> u32 {
        self.aggregate_stats(store_id).total
    }

    /// Pick the store under `prefix` with the most terminal rows (done+failed), for cold-start reads.
    pub fn best_store_key_with_items_under_prefix(&self, prefix: &str) -> Option<String> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return None;
        }
        let ids: Vec<String> = if let Some(db) = self.persistence.read().clone() {
            db.list_store_ids_with_rows(prefix).unwrap_or_else(|e| {
                log::warn!("work_items: list_store_ids_with_rows failed prefix={prefix}: {e}");
                Vec::new()
            })
        } else {
            self.inner
                .read()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect()
        };
        let mut best: Option<(String, u32)> = None;
        for id in ids {
            let stats = self.store_stats(&id);
            if stats.total == 0 {
                continue;
            }
            let terminal = stats.done + stats.failed;
            let replace = best
                .as_ref()
                .map(|(_, t)| terminal > *t)
                .unwrap_or(true);
            if replace {
                best = Some((id.clone(), terminal));
            }
        }
        best.map(|(id, _)| id)
    }

    pub fn all_terminal_have_summary(&self, store_id: &str) -> bool {
        let map = self.store_map(store_id);
        for item in map.values() {
            if !item.status.is_terminal() {
                continue;
            }
            if item.status == WorkItemStatus::Cancelled {
                continue;
            }
            let has = item
                .result_json
                .as_deref()
                .and_then(super::model::result_summary_from_json)
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            if !has && item.status == WorkItemStatus::Done {
                return false;
            }
        }
        true
    }

    pub fn items_in_store(&self, store_id: &str) -> Vec<WorkItem> {
        let mut items: Vec<WorkItem> = self.store_map(store_id).into_values().collect();
        items.sort_by_key(|i| i.seq);
        items
    }

    pub fn list_store(
        &self,
        store_id: &str,
        offset: u32,
        limit: u32,
    ) -> (Vec<WorkItem>, u32) {
        let mut items: Vec<WorkItem> = self.store_map(store_id).into_values().collect();
        items.sort_by_key(|i| i.seq);
        let total = items.len() as u32;
        let start = offset.min(total) as usize;
        let end = offset.saturating_add(limit).min(total) as usize;
        (items[start..end].to_vec(), total)
    }

    pub fn seed_bulk(&self, store_id: &str, drafts: Vec<WorkItemDraft>) -> Result<SeedOutcome> {
        if drafts.len() > 50_000 {
            return Err(anyhow!("work_items: bulk seed exceeds 50000 rows"));
        }
        let now = now_ms();
        let mut seeded = 0u32;
        for draft in drafts {
            let payload = payload_with_target_key(draft.payload, draft.target_key.clone());
            let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
            if let Some(ref key) = draft.target_key {
                if self.target_key_exists(store_id, key)? {
                    return Err(anyhow!("duplicate_target_key: {key}"));
                }
            }
            let seq = self.next_seq(store_id);
            let id = work_item_id(store_id, seq);
            let item = WorkItem {
                id,
                store_id: store_id.to_string(),
                seq,
                status: WorkItemStatus::Pending,
                title: draft.title,
                payload_json,
                retry_count: 0,
                max_retries: 2,
                result_ref: None,
                result_json: None,
                error_message: None,
                created_at_ms: now,
                updated_at_ms: now,
                started_at_ms: None,
                finished_at_ms: None,
            };
            self.insert_item(store_id, item);
            seeded += 1;
        }
        log::info!("work_items: bulk seeded store_id={store_id} count={seeded}");
        Ok(SeedOutcome { seeded })
    }

    // --- v4 compatibility aliases (batch_id ignored) ---

    pub fn replace_campaign(&self, store_id: &str) -> Result<()> {
        self.replace_store(store_id)
    }

    pub fn seed_batch_from_values(
        &self,
        store_id: &str,
        _batch_id: &str,
        values: &[Value],
    ) -> Result<SeedOutcome> {
        self.seed_from_values(store_id, values)
    }

    pub fn seed_batch_bulk(
        &self,
        store_id: &str,
        _batch_id: &str,
        drafts: Vec<WorkItemDraft>,
    ) -> Result<SeedOutcome> {
        self.seed_bulk(store_id, drafts)
    }

    pub fn apply_delta_legacy(
        &self,
        store_id: &str,
        _batch_id: &str,
        delta: WorkItemDelta,
    ) -> Result<WorkItem> {
        self.apply_delta(store_id, delta)
    }

    pub fn claim_legacy(
        &self,
        store_id: &str,
        _batch_id: &str,
        claim: WorkItemClaim,
        quota: u32,
    ) -> Result<WorkItem> {
        self.claim(store_id, claim, quota)
    }

    pub fn batch_stats(&self, store_id: &str, _batch_id: &str) -> BatchStats {
        self.store_stats(store_id)
    }

    pub fn campaign_stats(&self, store_id: &str) -> StoreStats {
        self.aggregate_stats(store_id)
    }

    pub fn batch_has_done(&self, store_id: &str, _batch_id: &str) -> bool {
        self.store_has_done(store_id)
    }

    pub fn batch_total(&self, store_id: &str, _batch_id: &str) -> u32 {
        self.store_total(store_id)
    }

    pub fn count_campaign(&self, store_id: &str) -> u32 {
        self.count_store(store_id)
    }

    pub fn items_in_batch(&self, store_id: &str, _batch_id: &str) -> Vec<WorkItem> {
        self.items_in_store(store_id)
    }

    pub fn list_batch(
        &self,
        store_id: &str,
        _batch_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> (Vec<WorkItem>, u32) {
        self.list_store(store_id, offset, limit)
    }
}

impl Default for WorkItemStore {
    fn default() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            persistence: RwLock::new(None),
        }
    }
}

pub fn delta_from_value(v: &Value) -> Option<WorkItemDelta> {
    let id = match v.get("id")? {
        Value::Number(n) => n.as_i64()?.to_string(),
        Value::String(s) => s.trim().to_string(),
        _ => return None,
    };
    if id.is_empty() {
        return None;
    }
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .and_then(WorkItemStatus::from_str_loose)
        .unwrap_or(WorkItemStatus::Done);
    Some(WorkItemDelta {
        id,
        status,
        result_summary: v
            .get("result_summary")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        result_ref: v
            .get("result_ref")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        error_message: v
            .get("error_message")
            .and_then(|x| x.as_str())
            .map(str::to_string),
    })
}

pub fn claim_from_value(v: &Value) -> Option<WorkItemClaim> {
    let target_key = v
        .get("target_key")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&target_key)
        .to_string();
    let payload = v.get("payload").cloned().unwrap_or(Value::Null);
    let status = v
        .get("status")
        .and_then(|x| x.as_str())
        .and_then(WorkItemStatus::from_str_loose)
        .unwrap_or(WorkItemStatus::InProgress);
    Some(WorkItemClaim {
        target_key,
        title,
        payload,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn seed_and_delta_updates_stats() {
        let store = WorkItemStore::new();
        let key = "conv-wi";
        store
            .seed_from_values(
                key,
                &[
                    json!({"title": "App1"}),
                    json!({"title": "App2"}),
                    json!({"title": "App3"}),
                ],
            )
            .expect("seed");
        let stats = store.store_stats(key);
        assert_eq!(stats.total, 3);
        assert_eq!(stats.done, 0);

        let map = store.store_map(key);
        let first_id = map.values().next().expect("item").id.clone();
        store
            .apply_delta(
                key,
                WorkItemDelta {
                    id: first_id.clone(),
                    status: WorkItemStatus::Done,
                    result_summary: Some("opened".into()),
                    result_ref: None,
                    error_message: None,
                },
            )
            .expect("delta");
        let stats = store.store_stats(key);
        assert_eq!(stats.done, 1);
        assert_eq!(stats.total, 3);
    }

    #[test]
    fn apply_delta_accepts_numeric_id_and_json_integer() {
        let store = WorkItemStore::new();
        let key = "conv-num";
        store
            .seed_from_values(key, &[json!({"title": "A"}), json!({"title": "B"})])
            .expect("seed");
        store
            .apply_delta(
                key,
                WorkItemDelta {
                    id: "1".into(),
                    status: WorkItemStatus::Done,
                    result_summary: Some("ok".into()),
                    result_ref: None,
                    error_message: None,
                },
            )
            .expect("by string seq");
        assert_eq!(store.store_stats(key).done, 1);

        let delta = delta_from_value(&json!({
            "id": 2,
            "status": "done",
            "result_summary": "also ok"
        }))
        .expect("parse int id");
        store.apply_delta(key, delta).expect("by json int");
        assert_eq!(store.store_stats(key).done, 2);
    }

    #[test]
    fn apply_delta_resolves_legacy_wi_prefix_id() {
        let store = WorkItemStore::new();
        let key = "conv-legacy";
        store
            .seed_from_values(key, &[json!({"title": "City"})])
            .expect("seed");
        let mut map = store.store_map(key);
        let legacy_key = map.keys().next().unwrap().clone();
        // Simulate pre-migration row still keyed by legacy id in memory.
        let mut item = map.remove(&legacy_key).unwrap();
        item.id = format!("wi_{key}_{:06}", item.seq);
        map.insert(item.id.clone(), item);
        store.inner.write().insert(key.to_string(), map);

        store
            .apply_delta(
                key,
                WorkItemDelta {
                    id: "1".into(),
                    status: WorkItemStatus::Done,
                    result_summary: Some("legacy path".into()),
                    result_ref: None,
                    error_message: None,
                },
            )
            .expect("legacy resolve");
        assert_eq!(store.store_stats(key).done, 1);
        assert_eq!(
            store.store_map(key).get("1").expect("canonical id").status,
            WorkItemStatus::Done
        );
    }

    #[test]
    fn inject_window_caps_detail_count() {
        let store = WorkItemStore::new();
        let key = "conv-big";
        let values: Vec<Value> = (1..=30)
            .map(|i| json!({"title": format!("item {i}")}))
            .collect();
        store.seed_from_values(key, &values).expect("seed");
        let map = store.store_map(key);
        for (idx, item) in map.values().enumerate().take(10) {
            let _ = store.apply_delta(
                key,
                WorkItemDelta {
                    id: item.id.clone(),
                    status: WorkItemStatus::Done,
                    result_summary: Some(format!("done {idx}")),
                    result_ref: None,
                    error_message: None,
                },
            );
        }
        let window = store.inject_window(key);
        assert!(window.len() <= 6);
    }
}
