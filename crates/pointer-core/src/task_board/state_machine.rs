//! Item and meta status transitions.

use super::model::{BoardDocument, BoardItem, ItemStatus};
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
            row.id == *dep
                && matches!(
                    row.status,
                    ItemStatus::Done | ItemStatus::Cancelled
                )
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

pub fn count_incomplete(doc: &BoardDocument) -> usize {
    doc.global_milestones
        .iter()
        .filter(|i| {
            !matches!(
                i.status,
                ItemStatus::Done | ItemStatus::Cancelled
            )
        })
        .count()
}
