//! Work item seeding and patch side-effects during task_board apply (v4).

use super::args::work_item_mode_from_args;
use super::model::{BoardDocument, BoardItem, BoardMeta, WorkItemMode, MAX_BOARD_ROWS};
use super::work_item::{
    claim_from_value, delta_from_value, drafts_from_source_value, work_items_source_path_from_value,
    WorkItemStore, MAX_INLINE_SEED,
};
use anyhow::{anyhow, Result};
use serde_json::Value;

pub fn work_items_enabled_from_args(args: &Value) -> bool {
    args.get("_task_board_work_items_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn workspace_root_from_args(args: &Value) -> &str {
    args.get("_workspace_root")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
}

pub fn doc_has_work_item_milestones(doc: &BoardDocument) -> bool {
    doc.has_work_items()
}

pub fn validate_board_row_count(rows: usize, method: &str) -> Result<()> {
    if rows > MAX_BOARD_ROWS {
        return Err(anyhow!(
            "task_board:{method} board exceeds MAX_BOARD_ROWS ({MAX_BOARD_ROWS})"
        ));
    }
    Ok(())
}

pub fn apply_meta_work_item_mode(doc: &mut BoardDocument, args: &Value) {
    if let Some(mode_s) = work_item_mode_from_args(args) {
        doc.meta.work_item_mode = match mode_s.to_ascii_lowercase().as_str() {
            "enumerated" => Some(WorkItemMode::Enumerated),
            "dynamic" => Some(WorkItemMode::Dynamic),
            _ => doc.meta.work_item_mode,
        };
    }
    if let Some(q) = super::args::dynamic_quota_from_args(args) {
        doc.meta.dynamic_quota = Some(q);
    }
}

fn args_has_work_items_source(args: &Value) -> bool {
    args.get("work_items_source")
        .is_some_and(|v| !v.is_null())
}

fn args_has_inline_work_items(args: &Value) -> bool {
    args.get("work_items")
        .and_then(|v| v.as_array())
        .is_some_and(|a| !a.is_empty())
}

/// Reject incompatible Type2 work_item_mode / seed combinations on init.
pub fn validate_work_item_init(
    args: &Value,
    doc: &BoardDocument,
    work_items_enabled: bool,
) -> Result<()> {
    let mode = doc.meta.work_item_mode;
    let has_source = args_has_work_items_source(args);
    let has_inline = args_has_inline_work_items(args);

    if has_source && mode != Some(WorkItemMode::Enumerated) {
        return Err(anyhow!(
            "work_items: work_items_source requires work_item_mode enumerated (known list from file)"
        ));
    }

    if let Some(WorkItemMode::Dynamic) = mode {
        if has_source {
            return Err(anyhow!("work_items: dynamic mode cannot use work_items_source"));
        }
        if has_inline {
            return Err(anyhow!(
                "work_items: dynamic mode cannot use inline work_items[] seed"
            ));
        }
        if work_items_enabled && doc.meta.dynamic_quota.unwrap_or(0) == 0 {
            return Err(anyhow!("work_items: dynamic mode requires dynamic_quota"));
        }
    }

    if let Some(WorkItemMode::Enumerated) = mode {
        if work_items_enabled && !has_source && !has_inline {
            return Err(anyhow!(
                "work_items: enumerated mode requires work_items_source or work_items[] on init"
            ));
        }
    }

    Ok(())
}

/// Seed work_items on **init only** from top-level `work_items` / `work_items_source`.
pub fn seed_work_items_on_init(
    store_key: &str,
    doc: &mut BoardDocument,
    args: &Value,
    work_items: &WorkItemStore,
    work_items_enabled: bool,
    workspace_root: &str,
) -> Result<u32> {
    doc.meta.work_items_source_path = None;
    doc.meta.work_items_seeded_rows = None;
    if !work_items_enabled {
        return Ok(0);
    }
    if !doc.has_work_items() {
        return Ok(0);
    }
    work_items.replace_store(store_key)?;
    if let Some(source) = args.get("work_items_source") {
        let resolved = work_items_source_path_from_value(source, workspace_root)?;
        let drafts = drafts_from_source_value(source, workspace_root)?;
        let outcome = work_items.seed_bulk(store_key, drafts)?;
        doc.meta.work_items_source_path = Some(resolved.display().to_string());
        doc.meta.work_items_seeded_rows = Some(outcome.seeded);
        log::info!(
            "work_items: seeded from source path={} rows={}",
            resolved.display(),
            outcome.seeded
        );
        return Ok(outcome.seeded);
    }
    let Some(arr) = args.get("work_items").and_then(|v| v.as_array()) else {
        return Ok(0);
    };
    if arr.len() > MAX_INLINE_SEED {
        return Err(anyhow!(
            "work_items: inline seed exceeds MAX_INLINE_SEED ({MAX_INLINE_SEED})"
        ));
    }
    let outcome = work_items.seed_from_values(store_key, arr)?;
    if outcome.seeded > 0 {
        doc.meta.work_items_seeded_rows = Some(outcome.seeded);
    }
    Ok(outcome.seeded)
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
    if doc.has_work_items() {
        let atomic = work_items.count_store(store_key);
        if atomic != expected_total {
            log::warn!(
                "task_board:{method} expected_total={expected_total} but work_items count={atomic}"
            );
        }
        return Ok(());
    }
    let rows = doc.global_milestones.len();
    if rows as u32 != expected_total {
        return Err(anyhow!(
            "task_board:{method} expected exactly {expected_total} item(s), got {rows}"
        ));
    }
    Ok(())
}

pub fn apply_work_item_patch_fields(
    store_key: &str,
    doc: &BoardDocument,
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

    if doc.is_dynamic_work_items() {
        if patch_v.get("work_item_delta").is_some() && patch_v.get("work_item_claim").is_some() {
            return Err(anyhow!("work_items: use work_item_delta or work_item_claim, not both"));
        }
        if let Some(claim_v) = patch_v.get("work_item_claim") {
            let claim = claim_from_value(claim_v)
                .ok_or_else(|| anyhow!("work_items: invalid work_item_claim"))?;
            let quota = doc.meta.dynamic_quota.unwrap_or(0);
            if quota == 0 {
                return Err(anyhow!("work_items: dynamic mode missing dynamic_quota"));
            }
            let item = work_items.claim(store_key, claim, quota)?;
            return Ok(Some(format!("claimed {} ({})", item.id, item.title)));
        }
    } else if doc.is_enumerated_work_items() {
        if patch_v.get("work_item_claim").is_some() {
            return Err(anyhow!("claim_on_enumerated_row"));
        }
    }

    if let Some(delta_v) = patch_v.get("work_item_delta") {
        if !doc.has_work_items() {
            return Err(anyhow!("work_items: work_item_delta on board without work_item_mode"));
        }
        let delta = delta_from_value(delta_v)
            .ok_or_else(|| anyhow!("work_items: invalid work_item_delta"))?;
        work_items.apply_delta(store_key, delta)?;
    }

    let stats = work_items.store_stats(store_key);
    Ok(Some(stats.progress_label()))
}

pub fn exec_met(doc: &BoardDocument, work_items: &WorkItemStore, store_key: &str) -> bool {
    if !doc.has_work_items() {
        return false;
    }
    let stats = work_items.store_stats(store_key);
    let expected = doc.meta.expected_total.unwrap_or(stats.total);
    if doc.is_enumerated_work_items() {
        if stats.pending > 0 || stats.in_progress > 0 {
            return false;
        }
        let terminal = stats.done + stats.failed;
        if terminal != expected {
            return false;
        }
        return work_items.all_terminal_have_summary(store_key);
    }
    if stats.in_progress > 0 {
        return false;
    }
    let quota = doc.meta.dynamic_quota.unwrap_or(0);
    stats.done + stats.failed >= quota
}

pub fn reset_item_milestones(doc: &mut BoardDocument) {
    for row in &mut doc.item_milestones {
        if row.id.starts_with("deliver_") {
            continue;
        }
        row.status = super::model::ItemStatus::Pending;
        row.remark = None;
    }
}

pub fn maybe_auto_complete_g_exec(
    doc: &mut BoardDocument,
    work_items: &WorkItemStore,
    store_key: &str,
) {
    if !exec_met(doc, work_items, store_key) {
        return;
    }
    if let Some(row) = doc.global_milestones.iter_mut().find(|r| r.id == "g_exec") {
        if row.status != super::model::ItemStatus::Done {
            row.status = super::model::ItemStatus::Done;
            log::info!("task_board: auto g_exec→done store_key={store_key}");
        }
    }
    if let Some(row) = doc.global_milestones.iter_mut().find(|r| r.id == "g_deliver") {
        if row.status == super::model::ItemStatus::Pending {
            row.status = super::model::ItemStatus::Ready;
        }
    }
}

pub fn patch_rejects_g_deliver_when_blocked(
    global_rows: &[BoardItem],
    row_id: &str,
    new_status: super::model::ItemStatus,
    has_work_items: bool,
    work_items: &WorkItemStore,
    store_key: &str,
) -> Result<()> {
    if !has_work_items || row_id != "g_deliver" {
        return Ok(());
    }
    if !matches!(
        new_status,
        super::model::ItemStatus::InProgress | super::model::ItemStatus::Done
    ) {
        return Ok(());
    }
    let g_exec_done = global_rows
        .iter()
        .find(|r| r.id == "g_exec")
        .map(|r| r.status == super::model::ItemStatus::Done)
        .unwrap_or(false);
    let doc_stub = BoardDocument {
        version: super::model::BOARD_VERSION,
        task_id: String::new(),
        meta: BoardMeta {
            work_item_mode: Some(WorkItemMode::Enumerated),
            ..BoardMeta::default()
        },
        global_context: Default::default(),
        global_milestones: global_rows.to_vec(),
        item_milestones: Vec::new(),
    };
    if !g_exec_done && !exec_met(&doc_stub, work_items, store_key) {
        return Err(anyhow!("g_deliver_blocked: g_exec not terminal"));
    }
    let stats = work_items.store_stats(store_key);
    if stats.in_progress > 0 {
        return Err(anyhow!("g_deliver_blocked: work_item still in_progress"));
    }
    Ok(())
}

pub fn patch_rejects_g_exec_done_when_not_met(
    global_rows: &[BoardItem],
    row_id: &str,
    new_status: super::model::ItemStatus,
    has_work_items: bool,
    work_items: &WorkItemStore,
    store_key: &str,
) -> Result<()> {
    if !has_work_items || row_id != "g_exec" || new_status != super::model::ItemStatus::Done {
        return Ok(());
    }
    let doc_stub = BoardDocument {
        version: super::model::BOARD_VERSION,
        task_id: String::new(),
        meta: BoardMeta {
            work_item_mode: Some(WorkItemMode::Enumerated),
            ..BoardMeta::default()
        },
        global_context: Default::default(),
        global_milestones: global_rows.to_vec(),
        item_milestones: Vec::new(),
    };
    if exec_met(&doc_stub, work_items, store_key) {
        return Ok(());
    }
    Err(anyhow!("g_exec_not_terminal"))
}

/// Legacy stub — v4 removed B42.
pub fn b42_enforced_from_args(_args: &Value) -> bool {
    false
}

pub fn patch_rejects_v3_delta_fields(_patch_v: &Value, _row: &BoardItem, _b42: bool) -> Result<()> {
    Ok(())
}

pub fn milestone_done_has_work_item_evidence(
    store_key: &str,
    _item: &BoardItem,
    work_items: &WorkItemStore,
) -> bool {
    work_items.store_has_done(store_key)
        || work_items.store_stats(store_key).is_batch_terminal()
}

#[cfg(test)]
mod init_validation_tests {
    use super::*;
    use crate::task_board::model::{BoardDocument, WorkItemMode};
    use serde_json::json;

    fn doc_with_mode(mode: WorkItemMode) -> BoardDocument {
        let mut doc = BoardDocument::empty_for_store_key("k");
        doc.meta.work_item_mode = Some(mode);
        doc
    }

    #[test]
    fn rejects_dynamic_with_work_items_source() {
        let mut doc = doc_with_mode(WorkItemMode::Dynamic);
        doc.meta.dynamic_quota = Some(50);
        let args = json!({
            "work_item_mode": "dynamic",
            "dynamic_quota": 50,
            "work_items_source": "/tmp/list.xlsx"
        });
        apply_meta_work_item_mode(&mut doc, &args);
        let err = validate_work_item_init(&args, &doc, true).unwrap_err();
        assert!(err.to_string().contains("work_items_source"));
    }

    #[test]
    fn rejects_enumerated_without_seed() {
        let mut doc = doc_with_mode(WorkItemMode::Enumerated);
        let args = json!({ "work_item_mode": "enumerated", "expected_total": 10 });
        apply_meta_work_item_mode(&mut doc, &args);
        let err = validate_work_item_init(&args, &doc, true).unwrap_err();
        assert!(err.to_string().contains("requires work_items_source"));
    }

    #[test]
    fn accepts_enumerated_with_source_path() {
        let mut doc = doc_with_mode(WorkItemMode::Enumerated);
        let args = json!({
            "work_item_mode": "enumerated",
            "expected_total": 10,
            "work_items_source": "/tmp/list.xlsx"
        });
        apply_meta_work_item_mode(&mut doc, &args);
        validate_work_item_init(&args, &doc, true).expect("ok");
    }

    #[test]
    fn rejects_source_without_enumerated_mode() {
        let doc = BoardDocument::empty_for_store_key("k");
        let args = json!({
            "work_item_mode": "dynamic",
            "dynamic_quota": 50,
            "work_items_source": "/tmp/list.xlsx"
        });
        let err = validate_work_item_init(&args, &doc, true).unwrap_err();
        assert!(err.to_string().contains("requires work_item_mode enumerated"));
    }
}
