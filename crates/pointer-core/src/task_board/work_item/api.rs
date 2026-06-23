//! Read-only work_items listing for UI pagination (APP + WEB).

use super::store::WorkItemStore;
use serde_json::{json, Value};

pub fn list_work_items_json(
    store: &WorkItemStore,
    store_id: &str,
    _batch_id: Option<&str>,
    offset: u32,
    limit: u32,
) -> Value {
    let limit = limit.clamp(1, 200);
    let (items, total) = store.list_store(store_id, offset, limit);
    let rows: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "store_id": i.store_id,
                "seq": i.seq,
                "title": i.title,
                "status": i.status.as_str(),
                "result_summary": i.result_json,
                "updated_at_ms": i.updated_at_ms,
            })
        })
        .collect();
    json!({
        "store_id": store_id,
        "offset": offset,
        "limit": limit,
        "total": total,
        "items": rows,
    })
}

pub fn work_item_stats_json(store: &WorkItemStore, store_id: &str, _batch_id: Option<&str>) -> Value {
    let s = store.store_stats(store_id);
    json!({
        "store_id": store_id,
        "total": s.total,
        "done": s.done,
        "failed": s.failed,
        "in_progress": s.in_progress,
        "pending": s.pending,
    })
}
