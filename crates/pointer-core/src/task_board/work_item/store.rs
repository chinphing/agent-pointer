//! In-memory work item store with optional SQLite write-through.

use super::model::{
    draft_from_value, merge_result_summary, now_ms, payload_with_target_key, target_key_from_payload,
    BatchStats, CampaignStats, SeedOutcome, WorkItem, WorkItemDraft, WorkItemStatus, MAX_INLINE_SEED,
    work_item_id,
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

    fn ensure_loaded(&self, campaign_id: &str) {
        if self.inner.read().contains_key(campaign_id) {
            return;
        }
        if let Some(db) = self.persistence.read().clone() {
            match db.load_campaign(campaign_id) {
                Ok(items) => {
                    let mut map = HashMap::new();
                    for item in items {
                        map.insert(item.id.clone(), item);
                    }
                    self.inner.write().insert(campaign_id.to_string(), map);
                }
                Err(e) => {
                    log::warn!("work_items: load failed campaign_id={campaign_id}: {e}");
                }
            }
        }
    }

    fn campaign_map(&self, campaign_id: &str) -> HashMap<String, WorkItem> {
        self.ensure_loaded(campaign_id);
        self.inner
            .read()
            .get(campaign_id)
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

    fn insert_item(&self, campaign_id: &str, item: WorkItem) {
        self.inner
            .write()
            .entry(campaign_id.to_string())
            .or_default()
            .insert(item.id.clone(), item.clone());
        self.persist_item(&item);
    }

    fn next_seq(&self, campaign_id: &str) -> i64 {
        let map = self.campaign_map(campaign_id);
        map.values().map(|i| i.seq).max().unwrap_or(0) + 1
    }

    pub fn replace_campaign(&self, campaign_id: &str) -> Result<()> {
        self.inner.write().remove(campaign_id);
        if let Some(db) = self.persistence.read().clone() {
            db.delete_campaign(campaign_id)?;
        }
        Ok(())
    }

    pub fn seed_batch(
        &self,
        campaign_id: &str,
        batch_id: &str,
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
                if self.target_key_exists(campaign_id, key)? {
                    return Err(anyhow!("duplicate_target_key: {key}"));
                }
            }
            let seq = self.next_seq(campaign_id);
            let id = work_item_id(campaign_id, seq);
            let item = WorkItem {
                id,
                campaign_id: campaign_id.to_string(),
                batch_id: batch_id.to_string(),
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
            self.insert_item(campaign_id, item);
            seeded += 1;
        }
        log::info!(
            "work_items: seeded campaign_id={campaign_id} batch_id={batch_id} count={seeded}"
        );
        Ok(SeedOutcome {
            seeded,
            batch_id: batch_id.to_string(),
        })
    }

    pub fn seed_batch_from_values(
        &self,
        campaign_id: &str,
        batch_id: &str,
        values: &[Value],
    ) -> Result<SeedOutcome> {
        let drafts: Vec<WorkItemDraft> = values.iter().filter_map(draft_from_value).collect();
        if drafts.is_empty() && !values.is_empty() {
            return Err(anyhow!("work_items: no valid work_items entries in seed"));
        }
        self.seed_batch(campaign_id, batch_id, drafts)
    }

    pub fn apply_delta(
        &self,
        campaign_id: &str,
        batch_id: &str,
        delta: WorkItemDelta,
    ) -> Result<WorkItem> {
        let mut map = self.campaign_map(campaign_id);
        let item = map
            .get_mut(&delta.id)
            .ok_or_else(|| anyhow!("work_item_not_found: {}", delta.id))?;
        if item.batch_id != batch_id {
            return Err(anyhow!(
                "work_item_not_found: {} not in batch {batch_id}",
                delta.id
            ));
        }
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
        let out = item.clone();
        self.inner
            .write()
            .entry(campaign_id.to_string())
            .or_default()
            .insert(out.id.clone(), out.clone());
        self.persist_item(&out);
        Ok(out)
    }

    pub fn claim(
        &self,
        campaign_id: &str,
        batch_id: &str,
        claim: WorkItemClaim,
        quota: u32,
    ) -> Result<WorkItem> {
        let stats = self.batch_stats(campaign_id, batch_id);
        let active = stats.done + stats.failed + stats.in_progress;
        if active >= quota {
            return Err(anyhow!("quota_exhausted"));
        }
        let key = claim.target_key.trim();
        if key.is_empty() {
            return Err(anyhow!("work_items: claim requires target_key"));
        }
        if self.target_key_exists(campaign_id, key)? {
            return Err(anyhow!("duplicate_target_key: {key}"));
        }
        let now = now_ms();
        let seq = self.next_seq(campaign_id);
        let id = work_item_id(campaign_id, seq);
        let payload = payload_with_target_key(claim.payload, Some(key.to_string()));
        let item = WorkItem {
            id,
            campaign_id: campaign_id.to_string(),
            batch_id: batch_id.to_string(),
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
        self.insert_item(campaign_id, item.clone());
        log::info!(
            "work_items: claim campaign_id={campaign_id} batch_id={batch_id} id={} target_key={key}",
            item.id
        );
        Ok(item)
    }

    pub fn target_key_exists(&self, campaign_id: &str, target_key: &str) -> Result<bool> {
        self.ensure_loaded(campaign_id);
        if let Some(map) = self.inner.read().get(campaign_id) {
            for item in map.values() {
                if target_key_from_payload(&item.payload_json).as_deref() == Some(target_key) {
                    return Ok(true);
                }
            }
        }
        if let Some(db) = self.persistence.read().clone() {
            return db.target_key_exists(campaign_id, target_key);
        }
        Ok(false)
    }

    pub fn batch_stats(&self, campaign_id: &str, batch_id: &str) -> BatchStats {
        let map = self.campaign_map(campaign_id);
        let mut stats = BatchStats::default();
        for item in map.values().filter(|i| i.batch_id == batch_id) {
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

    pub fn campaign_stats(&self, campaign_id: &str) -> CampaignStats {
        let map = self.campaign_map(campaign_id);
        let mut stats = CampaignStats::default();
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

    pub fn batch_has_done(&self, campaign_id: &str, batch_id: &str) -> bool {
        self.batch_stats(campaign_id, batch_id).done > 0
    }

    pub fn inject_window(&self, campaign_id: &str, batch_id: &str) -> Vec<WorkItem> {
        let map = self.campaign_map(campaign_id);
        let mut batch_items: Vec<WorkItem> = map
            .values()
            .filter(|i| i.batch_id == batch_id)
            .cloned()
            .collect();
        batch_items.sort_by_key(|i| i.seq);

        let mut out = Vec::new();
        for item in batch_items
            .iter()
            .filter(|i| i.status == WorkItemStatus::InProgress)
            .take(WORK_ITEM_INJECT_IN_PROGRESS)
        {
            out.push(item.clone());
        }
        let mut recent_done: Vec<WorkItem> = batch_items
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
        for item in batch_items
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

    pub fn batch_total(&self, campaign_id: &str, batch_id: &str) -> u32 {
        self.batch_stats(campaign_id, batch_id).total
    }

    pub fn count_campaign(&self, campaign_id: &str) -> u32 {
        self.campaign_stats(campaign_id).total
    }

    pub fn items_in_batch(&self, campaign_id: &str, batch_id: &str) -> Vec<WorkItem> {
        let map = self.campaign_map(campaign_id);
        let mut items: Vec<WorkItem> = map
            .into_values()
            .filter(|i| i.batch_id == batch_id)
            .collect();
        items.sort_by_key(|i| i.seq);
        items
    }

    pub fn list_batch(
        &self,
        campaign_id: &str,
        batch_id: Option<&str>,
        offset: u32,
        limit: u32,
    ) -> (Vec<WorkItem>, u32) {
        let map = self.campaign_map(campaign_id);
        let mut items: Vec<WorkItem> = map
            .into_values()
            .filter(|i| batch_id.map(|b| i.batch_id == b).unwrap_or(true))
            .collect();
        items.sort_by_key(|i| i.seq);
        let total = items.len() as u32;
        let start = offset.min(total) as usize;
        let end = offset.saturating_add(limit).min(total) as usize;
        (items[start..end].to_vec(), total)
    }

    pub fn seed_batch_bulk(
        &self,
        campaign_id: &str,
        batch_id: &str,
        drafts: Vec<WorkItemDraft>,
    ) -> Result<SeedOutcome> {
        if drafts.len() > 50_000 {
            return Err(anyhow!("work_items: bulk seed exceeds 50000 rows"));
        }
        let now = now_ms();
        let mut seeded = 0u32;
        for draft in drafts {
            let payload = payload_with_target_key(draft.payload, draft.target_key.clone());
            let payload_json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
            if let Some(ref key) = draft.target_key {
                if self.target_key_exists(campaign_id, key)? {
                    return Err(anyhow!("duplicate_target_key: {key}"));
                }
            }
            let seq = self.next_seq(campaign_id);
            let id = work_item_id(campaign_id, seq);
            let item = WorkItem {
                id,
                campaign_id: campaign_id.to_string(),
                batch_id: batch_id.to_string(),
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
            self.insert_item(campaign_id, item);
            seeded += 1;
        }
        log::info!(
            "work_items: bulk seeded campaign_id={campaign_id} batch_id={batch_id} count={seeded}"
        );
        Ok(SeedOutcome {
            seeded,
            batch_id: batch_id.to_string(),
        })
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
    let id = v.get("id")?.as_str()?.trim().to_string();
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
            .seed_batch_from_values(
                key,
                "batch_a",
                &[
                    json!({"title": "App1"}),
                    json!({"title": "App2"}),
                    json!({"title": "App3"}),
                ],
            )
            .expect("seed");
        let stats = store.batch_stats(key, "batch_a");
        assert_eq!(stats.total, 3);
        assert_eq!(stats.done, 0);

        let map = store.campaign_map(key);
        let first_id = map.values().next().expect("item").id.clone();
        store
            .apply_delta(
                key,
                "batch_a",
                WorkItemDelta {
                    id: first_id.clone(),
                    status: WorkItemStatus::Done,
                    result_summary: Some("opened".into()),
                    result_ref: None,
                    error_message: None,
                },
            )
            .expect("delta");
        let stats = store.batch_stats(key, "batch_a");
        assert_eq!(stats.done, 1);
        assert_eq!(stats.total, 3);
    }

    #[test]
    fn inject_window_caps_detail_count() {
        let store = WorkItemStore::new();
        let key = "conv-big";
        let values: Vec<Value> = (1..=30)
            .map(|i| json!({"title": format!("item {i}")}))
            .collect();
        store
            .seed_batch_from_values(key, "batch", &values)
            .expect("seed");
        let map = store.campaign_map(key);
        for (idx, item) in map.values().enumerate().take(10) {
            let _ = store.apply_delta(
                key,
                "batch",
                WorkItemDelta {
                    id: item.id.clone(),
                    status: WorkItemStatus::Done,
                    result_summary: Some(format!("done {idx}")),
                    result_ref: None,
                    error_message: None,
                },
            );
        }
        let window = store.inject_window(key, "batch");
        assert!(window.len() <= 6);
    }
}
