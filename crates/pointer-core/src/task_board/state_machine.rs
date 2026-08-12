//! Item and meta status transitions.

use super::loop_milestones;
use super::model::{BoardDocument, BoardItem, ItemStatus, MetaStatus};
use anyhow::{anyhow, Result};

pub fn validate_item_transition(from: ItemStatus, to: ItemStatus) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let ok = match (from, to) {
        (ItemStatus::Done, ItemStatus::InProgress)
        | (ItemStatus::Done, ItemStatus::Pending)
        | (ItemStatus::Done, ItemStatus::Ready) => false,
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(anyhow!(
            "task_board: invalid status transition {from:?} → {to:?}"
        ))
    }
}

pub fn dependencies_satisfied(doc: &BoardDocument, item: &BoardItem) -> bool {
    dependencies_satisfied_rows(&doc.global_milestones, item)
}

pub fn dependencies_satisfied_rows(global: &[BoardItem], item: &BoardItem) -> bool {
    if item.depends_on.is_empty() {
        return true;
    }
    item.depends_on.iter().all(|dep| {
        global.iter().any(|row| {
            row.id == *dep && matches!(row.status, ItemStatus::Done | ItemStatus::Cancelled)
        })
    })
}

pub fn mark_ready_pending_rows(doc: &mut BoardDocument) {
    let ready_ids: Vec<String> = doc
        .global_milestones
        .iter()
        .filter(|item| item.status == ItemStatus::Pending && dependencies_satisfied(doc, item))
        .map(|i| i.id.clone())
        .collect();
    for item in doc.global_milestones.iter_mut() {
        if ready_ids.contains(&item.id) {
            item.status = ItemStatus::Ready;
        }
    }
}

/// Keep exactly one active row when the board is still running.
///
/// Type1 boards historically stayed at `ready` until the model patched
/// `in_progress`, so the UI showed no spinner while work continued. Host now
/// promotes the first eligible `ready`/`pending` row (same idea as loop `wi_*`
/// bootstrap). Loop boards delegate to the `wi_*` ladder.
pub fn ensure_single_milestone_in_progress(doc: &mut BoardDocument) {
    if matches!(
        doc.meta.status,
        MetaStatus::Completed | MetaStatus::Failed
    ) {
        return;
    }
    if loop_milestones::is_loop_milestone_board(doc) {
        loop_milestones::ensure_single_loop_item_in_progress(doc);
        return;
    }
    if doc
        .global_milestones
        .iter()
        .any(|r| r.status == ItemStatus::InProgress)
    {
        return;
    }
    // Prefer ready (deps already cleared by mark_ready), then pending with deps.
    let promote_id = doc
        .global_milestones
        .iter()
        .find(|r| r.status == ItemStatus::Ready)
        .or_else(|| {
            doc.global_milestones.iter().find(|r| {
                r.status == ItemStatus::Pending && dependencies_satisfied(doc, r)
            })
        })
        .map(|r| r.id.clone());
    let Some(id) = promote_id else {
        return;
    };
    if let Some(row) = doc.global_milestones.iter_mut().find(|r| r.id == id) {
        row.status = ItemStatus::InProgress;
        log::info!(
            "task_board: ensure_single_milestone_in_progress promoted {id} → in_progress"
        );
    }
}

pub fn count_incomplete(doc: &BoardDocument) -> usize {
    doc.global_milestones
        .iter()
        .filter(|i| !matches!(i.status, ItemStatus::Done | ItemStatus::Cancelled))
        .count()
}
