//! Re-export sync finding via store (child → parent global_context).

use super::super::store::TaskBoardStore;
use anyhow::Result;
use serde_json::Value;

pub fn sync_global_finding(
    store: &TaskBoardStore,
    child_store_key: &str,
    finding: &str,
) -> Result<Value> {
    let args = serde_json::json!({ "finding": finding });
    let (body, _) = store.apply(child_store_key, "sync_finding", &args)?;
    Ok(body)
}
