//! Read-only work_items listing for UI pagination (APP + WEB).

use super::store::WorkItemStore;
use serde_json::{json, Value};

pub fn list_work_items_json(
    store: &WorkItemStore,
    campaign_id: &str,
    batch_id: Option<&str>,
    offset: u32,
    limit: u32,
) -> Value {
    let limit = limit.clamp(1, 200);
    let (items, total) = store.list_batch(campaign_id, batch_id, offset, limit);
    let rows: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "batch_id": i.batch_id,
                "seq": i.seq,
                "title": i.title,
                "status": i.status.as_str(),
                "result_summary": i.result_json,
                "updated_at_ms": i.updated_at_ms,
            })
        })
        .collect();
    json!({
        "campaign_id": campaign_id,
        "batch_id": batch_id,
        "offset": offset,
        "limit": limit,
        "total": total,
        "items": rows,
    })
}

pub fn work_item_stats_json(store: &WorkItemStore, campaign_id: &str, batch_id: Option<&str>) -> Value {
    let stats = if let Some(b) = batch_id {
        let s = store.batch_stats(campaign_id, b);
        json!({
            "campaign_id": campaign_id,
            "batch_id": b,
            "total": s.total,
            "done": s.done,
            "failed": s.failed,
            "in_progress": s.in_progress,
        })
    } else {
        let s = store.campaign_stats(campaign_id);
        json!({
            "campaign_id": campaign_id,
            "total": s.total,
            "done": s.done,
            "failed": s.failed,
            "in_progress": s.in_progress,
        })
    };
    stats
}
