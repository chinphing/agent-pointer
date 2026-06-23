//! Work item seeding and patch side-effects during task_board apply.

use super::model::{BoardDocument, BoardItem, MAX_BOARD_ROWS};
use super::work_item::{
    claim_from_value, delta_from_value, WorkItemStore, MAX_INLINE_SEED,
};
use anyhow::{anyhow, Result};
use serde_json::Value;

pub fn work_items_enabled_from_args(args: &Value) -> bool {
    args.get("_task_board_work_items_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn b42_enforced_from_args(args: &Value) -> bool {
    args.get("_task_board_b42_enforced")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn doc_has_work_item_milestones(doc: &BoardDocument) -> bool {
    doc.board.iter().any(|r| r.has_work_items())
}

pub fn validate_board_row_count(rows: usize, method: &str) -> Result<()> {
    if rows > MAX_BOARD_ROWS {
        return Err(anyhow!(
            "task_board:{method} board exceeds MAX_BOARD_ROWS ({MAX_BOARD_ROWS})"
        ));
    }
    Ok(())
}

pub fn seed_work_items_on_init_replace(
    store_key: &str,
    _doc: &BoardDocument,
    row_values: &[Value],
    work_items: &WorkItemStore,
    work_items_enabled: bool,
) -> Result<u32> {
    if !work_items_enabled {
        return Ok(0);
    }
    work_items.replace_campaign(store_key)?;
    let mut total_seeded = 0u32;
    for row_v in row_values {
        let Some(item) = BoardItem::from_value(row_v) else {
            continue;
        };
        if !item.is_enumerated_work_items() {
            continue;
        }
        let Some(arr) = row_v.get("work_items").and_then(|v| v.as_array()) else {
            continue;
        };
        if arr.len() > MAX_INLINE_SEED {
            return Err(anyhow!(
                "work_items: inline seed exceeds MAX_INLINE_SEED ({MAX_INLINE_SEED})"
            ));
        }
        let outcome = work_items.seed_batch_from_values(store_key, &item.id, arr)?;
        total_seeded += outcome.seeded;
    }
    Ok(total_seeded)
}

pub fn validate_expected_total_after_seed(
    doc: &BoardDocument,
    work_items: &WorkItemStore,
    store_key: &str,
    method: &str,
) -> Result<()> {
    let Some(expected_total) = doc.meta.expected_total else {
        return Ok(());
    };
    if doc_has_work_item_milestones(doc) {
        let atomic = work_items.count_campaign(store_key);
        if atomic != expected_total {
            log::warn!(
                "task_board:{method} expected_total={expected_total} but work_items count={atomic}"
            );
        }
        return Ok(());
    }
    let rows = doc.board.len();
    if rows as u32 != expected_total {
        return Err(anyhow!(
            "task_board:{method} expected exactly {expected_total} item(s), got {rows}"
        ));
    }
    Ok(())
}

pub fn apply_work_item_patch_fields(
    store_key: &str,
    prev: &BoardItem,
    patch_v: &Value,
    work_items: &WorkItemStore,
    work_items_enabled: bool,
) -> Result<Option<String>> {
    if !work_items_enabled {
        if patch_v.get("work_item_delta").is_some() || patch_v.get("work_item_claim").is_some() {
            return Err(anyhow!(
                "work_items: work_item_delta/claim disabled (task_board_work_items_enabled=false)"
            ));
        }
        return Ok(None);
    }

    if prev.is_dynamic_work_items() {
        if patch_v.get("work_item_delta").is_some() && patch_v.get("work_item_claim").is_some() {
            return Err(anyhow!("work_items: use work_item_delta or work_item_claim, not both"));
        }
        if let Some(claim_v) = patch_v.get("work_item_claim") {
            let claim = claim_from_value(claim_v)
                .ok_or_else(|| anyhow!("work_items: invalid work_item_claim"))?;
            let quota = prev.dynamic_quota.unwrap_or(0);
            if quota == 0 {
                return Err(anyhow!("work_items: dynamic milestone missing dynamic_quota"));
            }
            let item = work_items.claim(store_key, &prev.id, claim, quota)?;
            return Ok(Some(format!(
                "claimed {} ({})",
                item.id, item.title
            )));
        }
    } else if prev.is_enumerated_work_items() {
        if patch_v.get("work_item_claim").is_some() {
            return Err(anyhow!("claim_on_enumerated_row"));
        }
    }

    if let Some(delta_v) = patch_v.get("work_item_delta") {
        if !prev.has_work_items() {
            return Err(anyhow!(
                "work_items: work_item_delta on row without work_item_mode"
            ));
        }
        let delta = delta_from_value(delta_v)
            .ok_or_else(|| anyhow!("work_items: invalid work_item_delta"))?;
        work_items.apply_delta(store_key, &prev.id, delta)?;
    }

    let stats = work_items.batch_stats(store_key, &prev.id);
    Ok(Some(stats.progress_label()))
}

pub fn derive_row_progress(
    store_key: &str,
    item: &BoardItem,
    work_items: &WorkItemStore,
) -> Option<String> {
    if !item.has_work_items() {
        return item.progress.clone();
    }
    let stats = work_items.batch_stats(store_key, &item.id);
    if stats.total == 0 && item.is_dynamic_work_items() {
        let quota = item.dynamic_quota.unwrap_or(0);
        if quota > 0 {
            return Some(format!("{}/{} claimed", stats.done + stats.in_progress, quota));
        }
        return None;
    }
    Some(stats.progress_label())
}

pub fn patch_rejects_v3_delta_fields(patch_v: &Value, row: &BoardItem, b42: bool) -> Result<()> {
    if !b42 && !row.has_work_items() {
        return Ok(());
    }
    if !b42 {
        return Ok(());
    }
    const FORBIDDEN: &[&str] = &[
        "progress",
        "checkpoint",
        "validate_result_delta",
        "validate_results",
        "extract_result_delta",
        "extract_results",
    ];
    for key in FORBIDDEN {
        if patch_v.get(key).is_some() {
            return Err(anyhow!(
                "task_board: patch row {} sent {key}; use work_item_delta or status only",
                row.id
            ));
        }
    }
    Ok(())
}

pub fn milestone_done_has_work_item_evidence(
    store_key: &str,
    item: &BoardItem,
    work_items: &WorkItemStore,
) -> bool {
    if !item.has_work_items() {
        return false;
    }
    work_items.batch_has_done(store_key, &item.id)
        || work_items.batch_stats(store_key, &item.id).is_batch_terminal()
}
