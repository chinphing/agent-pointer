//! Restore work_items rows from board meta when SQLite is empty after reload.

use super::import::drafts_from_resolved_path;
use super::store::WorkItemStore;
use crate::task_board::store::TaskBoardStore;
use anyhow::Result;
use std::path::Path;

/// When board meta says items were seeded but the store is empty, re-import from
/// `meta.work_items_source_path` (e.g. after legacy SQLite column mismatch or cold start).
pub fn try_rehydrate_work_items_if_empty(
    board_store: &TaskBoardStore,
    work_items: &WorkItemStore,
    store_key: &str,
) -> Result<u32> {
    if work_items.count_store(store_key) > 0 {
        return Ok(0);
    }
    let doc = board_store.document(store_key);
    let expected = doc.meta.work_items_seeded_rows.unwrap_or(0);
    if expected == 0 {
        return Ok(0);
    }
    let Some(path_str) = doc
        .meta
        .work_items_source_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        log::warn!(
            "work_items: rehydrate skipped store_id={store_key} seeded_rows={expected} but no source path"
        );
        return Ok(0);
    };
    let path = Path::new(path_str);
    if !path.exists() {
        log::warn!(
            "work_items: rehydrate skipped store_id={store_key} source missing path={path_str}"
        );
        return Ok(0);
    }
    let drafts = drafts_from_resolved_path(path)?;
    if drafts.is_empty() {
        log::warn!(
            "work_items: rehydrate skipped store_id={store_key} source parsed 0 rows path={path_str}"
        );
        return Ok(0);
    }
    let outcome = work_items.seed_bulk(store_key, drafts)?;
    log::info!(
        "work_items: rehydrated store_id={store_key} rows={} from path={path_str}",
        outcome.seeded
    );
    Ok(outcome.seeded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_board::model::MetaStatus;
    use crate::task_board::store::TaskBoardStore;
    use std::io::Write;

    #[test]
    fn rehydrates_from_source_path_when_store_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cities.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "城市名称").unwrap();
        writeln!(f, "北京").unwrap();
        writeln!(f, "上海").unwrap();

        let board_store = TaskBoardStore::new();
        let work_items = board_store.work_items.as_ref();
        let store_key = "conv-rehydrate";
        let mut doc = crate::task_board::model::BoardDocument::empty_for_store_key(store_key);
        doc.meta.status = MetaStatus::Running;
        doc.meta.work_items_seeded_rows = Some(2);
        doc.meta.work_items_source_path = Some(path.display().to_string());
        board_store.save_document(store_key, doc);

        let n = try_rehydrate_work_items_if_empty(&board_store, work_items, store_key).expect("ok");
        assert_eq!(n, 2);
        assert_eq!(work_items.count_store(store_key), 2);
    }
}
