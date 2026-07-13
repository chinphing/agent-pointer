//! Loop/batch tasks as item rows in `global_milestones` (unified milestone patch).

use super::model::{BoardDocument, BoardItem, ItemStatus, MAX_BOARD_ROWS};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

const LOOP_ITEM_ID_PREFIX: &str = "wi_";

/// Shared per-item procedure on loop boards: read from `g_plan.plan` when present.
pub fn loop_shared_plan(doc: &BoardDocument) -> Option<String> {
    plan_index(doc).and_then(|i| {
        doc.global_milestones[i]
            .plan
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    })
}

fn deliver_index(doc: &BoardDocument) -> Option<usize> {
    doc.global_milestones
        .iter()
        .position(|r| r.id == "g_deliver")
}

fn plan_index(doc: &BoardDocument) -> Option<usize> {
    doc.global_milestones
        .iter()
        .position(|r| r.id == "g_plan")
}

fn legacy_exec_index(doc: &BoardDocument) -> Option<usize> {
    doc.global_milestones
        .iter()
        .position(|r| r.id == "g_exec")
}

/// `g_plan` and `g_deliver` present in order (loop shell without items).
pub fn loop_has_plan_and_deliver(doc: &BoardDocument) -> bool {
    match (plan_index(doc), deliver_index(doc)) {
        (Some(p), Some(d)) => p < d,
        _ => false,
    }
}

/// Indices `[start, end)` of loop item rows before `g_deliver`.
/// New boards: between `g_plan` and `g_deliver`. Legacy: between `g_exec` and `g_deliver`.
pub fn loop_item_range(doc: &BoardDocument) -> Option<(usize, usize)> {
    let deliver_idx = deliver_index(doc)?;
    let plan_idx = plan_index(doc)?;
    let mut start = plan_idx + 1;
    if let Some(exec_idx) = legacy_exec_index(doc) {
        if exec_idx >= start && exec_idx < deliver_idx {
            start = exec_idx + 1;
        }
    }
    if start >= deliver_idx {
        return None;
    }
    Some((start, deliver_idx))
}

pub fn loop_item_rows(doc: &BoardDocument) -> Vec<&BoardItem> {
    loop_item_range(doc)
        .map(|(start, end)| {
            doc.global_milestones[start..end]
                .iter()
                .filter(|r| r.id.starts_with(LOOP_ITEM_ID_PREFIX))
                .collect()
        })
        .unwrap_or_default()
}

pub fn loop_item_count(doc: &BoardDocument) -> usize {
    loop_item_rows(doc).len()
}

pub fn is_loop_item_id(doc: &BoardDocument, id: &str) -> bool {
    id.starts_with(LOOP_ITEM_ID_PREFIX)
        && loop_item_range(doc).is_some_and(|(start, end)| {
            doc.global_milestones[start..end]
                .iter()
                .any(|r| r.id == id)
        })
}

pub fn validate_board_row_count(rows: usize, method: &str) -> Result<()> {
    if rows > MAX_BOARD_ROWS {
        return Err(anyhow!(
            "task_board: {method} exceeds MAX_BOARD_ROWS ({MAX_BOARD_ROWS})"
        ));
    }
    Ok(())
}

/// New-style loop board: item rows live in `global_milestones`.
pub fn is_loop_milestone_board(doc: &BoardDocument) -> bool {
    loop_has_plan_and_deliver(doc) && loop_item_count(doc) > 0
}

pub fn loop_exec_met(doc: &BoardDocument) -> bool {
    let items = loop_item_rows(doc);
    !items.is_empty()
        && items.iter().all(|r| {
            matches!(
                r.status,
                ItemStatus::Done | ItemStatus::Failed | ItemStatus::Cancelled
            )
        })
}

pub fn loop_progress_json(doc: &BoardDocument) -> Option<Value> {
    if !is_loop_milestone_board(doc) {
        return None;
    }
    let items: Vec<&BoardItem> = loop_item_rows(doc);
    let total = items.len();
    let done = items
        .iter()
        .filter(|r| r.status == ItemStatus::Done)
        .count();
    let failed = items
        .iter()
        .filter(|r| r.status == ItemStatus::Failed)
        .count();
    let in_progress = items
        .iter()
        .filter(|r| r.status == ItemStatus::InProgress)
        .count();
    let pending = items
        .iter()
        .filter(|r| matches!(r.status, ItemStatus::Pending | ItemStatus::Ready))
        .count();
    let terminal = done + failed;
    let in_progress_id = items
        .iter()
        .find(|r| r.status == ItemStatus::InProgress)
        .map(|r| r.id.clone());
    let mut body = json!({
        "done": done,
        "failed": failed,
        "total": total,
        "in_progress": in_progress,
        "pending": pending,
        "progress": format!("{terminal}/{total}"),
    });
    if let Some(id) = in_progress_id {
        body["in_progress_id"] = json!(id);
    }
    Some(body)
}

pub fn sync_loop_meta(doc: &mut BoardDocument) {
    if !is_loop_milestone_board(doc) {
        return;
    }
    doc.meta.expected_total = Some(loop_item_count(doc) as u32);
}

fn dynamic_quota_from_args(args: &Value) -> Option<u32> {
    args.get("dynamic_quota")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .filter(|n| *n > 0)
}

fn make_loop_item_row(index: usize, title: &str) -> BoardItem {
    BoardItem {
        id: format!("{LOOP_ITEM_ID_PREFIX}{index}"),
        title: title.to_string(),
        status: ItemStatus::Pending,
        ..BoardItem::default()
    }
}

fn insert_loop_items(doc: &mut BoardDocument, items: Vec<BoardItem>) -> Result<()> {
    let deliver_idx = deliver_index(doc).ok_or_else(|| {
        anyhow!("task_board: loop init requires g_deliver in global_milestones")
    })?;
    let plan_idx = plan_index(doc).ok_or_else(|| {
        anyhow!("task_board: loop init requires g_plan in global_milestones")
    })?;
    if plan_idx >= deliver_idx {
        return Err(anyhow!(
            "task_board: loop init requires g_plan before g_deliver in global_milestones"
        ));
    }
    let (start, end) = if let Some((s, e)) = loop_item_range(doc) {
        (s, e)
    } else {
        (plan_idx + 1, plan_idx + 1)
    };
    let new_len = doc.global_milestones.len() - (end - start) + items.len();
    if new_len > MAX_BOARD_ROWS {
        return Err(anyhow!(
            "task_board: loop init exceeds MAX_BOARD_ROWS ({MAX_BOARD_ROWS})"
        ));
    }
    doc.global_milestones.splice(start..end, items);
    Ok(())
}

/// Expand loop item rows on init from `dynamic_quota` when wi_* rows are not already present.
pub fn expand_loop_milestones_on_init(doc: &mut BoardDocument, args: &Value) -> Result<()> {
    if loop_item_count(doc) > 0 {
        return Ok(());
    }
    if !loop_has_plan_and_deliver(doc) {
        return Ok(());
    }
    if legacy_exec_index(doc).is_some() {
        return Err(anyhow!(
            "task_board: loop init must omit g_exec; wi_* rows are the exec phase (use g_plan + g_deliver only)"
        ));
    }

    if let Some(quota) = dynamic_quota_from_args(args) {
        let rows: Vec<BoardItem> = (1..=quota)
            .map(|i| make_loop_item_row(i as usize, &format!("#{i}")))
            .collect();
        insert_loop_items(doc, rows)?;
        doc.meta.dynamic_quota = Some(quota);
    }

    Ok(())
}

pub fn validate_loop_milestone_init(doc: &BoardDocument, _args: &Value) -> Result<()> {
    if !is_loop_milestone_board(doc) {
        return Ok(());
    }
    if legacy_exec_index(doc).is_some() {
        return Err(anyhow!(
            "task_board: loop init must omit g_exec; wi_* rows are the exec phase (use g_plan + g_deliver only)"
        ));
    }
    if !loop_has_plan_and_deliver(doc) {
        return Err(anyhow!(
            "task_board: loop init requires g_plan and g_deliver in global_milestones"
        ));
    }
    let count = loop_item_count(doc);
    if count == 0 {
        return Err(anyhow!(
            "task_board: loop init requires wi_* item rows between g_plan and g_deliver"
        ));
    }
    for row in loop_item_rows(doc) {
        if row.id == "g_exec" {
            return Err(anyhow!(
                "task_board: loop init must not include g_exec in the item ladder"
            ));
        }
        let row_plan = row
            .plan
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if row_plan.is_none() && loop_shared_plan(doc).is_none() {
            return Err(anyhow!(
                "task_board: loop item {} requires plan or g_plan.plan (shared procedure)",
                row.id
            ));
        }
    }
    Ok(())
}

/// After init: g_plan done, first loop item in progress.
pub fn bootstrap_loop_milestone_board(doc: &mut BoardDocument) {
    if !is_loop_milestone_board(doc) {
        return;
    }
    for row in &mut doc.global_milestones {
        if row.id == "g_plan" && row.status != ItemStatus::Done {
            row.status = ItemStatus::Done;
        }
    }
    ensure_single_loop_item_in_progress(doc);
    sync_loop_meta(doc);
}

fn ensure_single_loop_item_in_progress(doc: &mut BoardDocument) {
    let Some((start, end)) = loop_item_range(doc) else {
        return;
    };
    let slice = &doc.global_milestones[start..end];
    if slice
        .iter()
        .any(|r| r.status == ItemStatus::InProgress)
    {
        return;
    }
    for row in &mut doc.global_milestones[start..end] {
        if row.id == "g_exec" {
            continue;
        }
        if matches!(row.status, ItemStatus::Pending | ItemStatus::Ready) {
            row.status = ItemStatus::InProgress;
            return;
        }
    }
}

fn advance_loop_item_after(doc: &mut BoardDocument, after_id: &str) {
    let Some((start, end)) = loop_item_range(doc) else {
        return;
    };
    let rows = &mut doc.global_milestones[start..end];
    let pos = rows.iter().position(|r| r.id == after_id);
    let Some(pos) = pos else {
        return;
    };
    for row in rows.iter_mut() {
        if row.status == ItemStatus::InProgress && row.id != after_id {
            row.status = ItemStatus::Pending;
        }
    }
    for row in rows.iter_mut().skip(pos + 1) {
        if row.id == "g_exec" {
            continue;
        }
        if matches!(row.status, ItemStatus::Pending | ItemStatus::Ready) {
            row.status = ItemStatus::InProgress;
            return;
        }
    }
}

pub fn maybe_auto_complete_loop_batch(doc: &mut BoardDocument) {
    if !loop_exec_met(doc) {
        return;
    }
    if let Some(row) = doc.global_milestones.iter_mut().find(|r| r.id == "g_deliver") {
        if row.status == ItemStatus::Pending {
            row.status = ItemStatus::Ready;
        }
    }
}

pub fn handle_loop_milestone_transition(
    doc: &mut BoardDocument,
    prev: &BoardItem,
    incoming: &BoardItem,
) {
    if prev.status == incoming.status {
        return;
    }
    if !is_loop_item_id(doc, &incoming.id) {
        return;
    }
    if matches!(
        incoming.status,
        ItemStatus::Done | ItemStatus::Failed | ItemStatus::Cancelled
    ) {
        advance_loop_item_after(doc, &incoming.id);
        maybe_auto_complete_loop_batch(doc);
        sync_loop_meta(doc);
    }
}

pub fn patch_rejects_g_deliver_when_loop_blocked(
    doc: &BoardDocument,
    row_id: &str,
    new_status: ItemStatus,
) -> Result<()> {
    if !is_loop_milestone_board(doc) || row_id != "g_deliver" {
        return Ok(());
    }
    if !matches!(new_status, ItemStatus::InProgress | ItemStatus::Done) {
        return Ok(());
    }
    if !loop_exec_met(doc) {
        return Err(anyhow!(
            "task_board: g_deliver blocked until all loop items are terminal"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_board::model::BoardDocument;

    fn loop_shell() -> BoardDocument {
        let mut doc = BoardDocument::empty_for_store_key("loop-test");
        doc.meta.goal = "loop test".into();
        doc.global_milestones = vec![
            BoardItem {
                id: "g_plan".into(),
                title: "Plan".into(),
                status: ItemStatus::Done,
                ..BoardItem::default()
            },
            BoardItem {
                id: "g_deliver".into(),
                title: "Deliver".into(),
                status: ItemStatus::Pending,
                ..BoardItem::default()
            },
        ];
        doc
    }

    #[test]
    fn expand_dynamic_quota_inserts_loop_rows_without_g_exec() {
        let mut doc = loop_shell();
        expand_loop_milestones_on_init(&mut doc, &json!({ "dynamic_quota": 2 }))
        .expect("expand");
        assert!(is_loop_milestone_board(&doc));
        assert_eq!(loop_item_count(&doc), 2);
        assert_eq!(doc.global_milestones.len(), 4);
        assert_eq!(doc.global_milestones[0].id, "g_plan");
        assert_eq!(doc.global_milestones[1].id, "wi_1");
        assert_eq!(doc.global_milestones[2].id, "wi_2");
        assert_eq!(doc.global_milestones[3].id, "g_deliver");
        assert!(doc.global_milestones[1].plan.is_none());
    }

    #[test]
    fn rejects_dynamic_quota_over_max_board_rows() {
        let mut doc = loop_shell();
        let err = expand_loop_milestones_on_init(&mut doc, &json!({ "dynamic_quota": 99 }))
            .expect_err("expand");
        assert!(err.to_string().contains("MAX_BOARD_ROWS"));
    }

    #[test]
    fn rejects_g_exec_on_new_dynamic_loop_init() {
        let mut doc = loop_shell();
        doc.global_milestones.insert(
            1,
            BoardItem {
                id: "g_exec".into(),
                title: "Exec".into(),
                status: ItemStatus::Pending,
                ..BoardItem::default()
            },
        );
        let err = expand_loop_milestones_on_init(&mut doc, &json!({ "dynamic_quota": 2 }))
        .expect_err("expand");
        assert!(err.to_string().contains("omit g_exec"));
    }

    #[test]
    fn loop_item_done_advances_next() {
        let mut doc = loop_shell();
        doc.global_milestones.insert(
            1,
            BoardItem {
                id: "wi_1".into(),
                title: "a".into(),
                status: ItemStatus::InProgress,
                plan: Some("steps".into()),
                ..BoardItem::default()
            },
        );
        doc.global_milestones.insert(
            2,
            BoardItem {
                id: "wi_2".into(),
                title: "b".into(),
                status: ItemStatus::Pending,
                plan: Some("steps".into()),
                ..BoardItem::default()
            },
        );
        let prev = doc.global_milestones[1].clone();
        let mut incoming = prev.clone();
        incoming.status = ItemStatus::Done;
        doc.global_milestones[1].status = ItemStatus::Done;
        handle_loop_milestone_transition(&mut doc, &prev, &incoming);
        assert_eq!(doc.global_milestones[1].status, ItemStatus::Done);
        assert_eq!(doc.global_milestones[2].status, ItemStatus::InProgress);
    }
}
