//! Pack parent context when dispatching a sub-agent.

use super::super::model::{BoardItem, BoardScope, ItemStatus};
use super::super::store::TaskBoardStore;
use anyhow::Result;

pub struct DispatchContext {
    pub parent_goal: String,
    pub global_findings: Vec<String>,
    pub milestone_title: String,
    pub milestone_validate: Option<String>,
}

pub fn dispatch_to_child(
    store: &TaskBoardStore,
    parent_key: &str,
    child_key: &str,
    sub_task_id: &str,
    milestone: Option<BoardItem>,
) -> Result<DispatchContext> {
    let parent = store.document(parent_key);
    let mut child = store.document(child_key);
    child.meta.scope = Some(BoardScope::Child);
    child.meta.parent_sub_task_id = Some(sub_task_id.to_string());
    child.meta.root_target = Some(sub_task_id.to_string());
    child.meta.parent_store_key = Some(parent_key.to_string());
    if child.meta.goal.is_empty() {
        child.meta.goal = parent
            .board
            .iter()
            .find(|i| i.id == sub_task_id)
            .map(|i| i.title.clone())
            .unwrap_or_else(|| sub_task_id.to_string());
    }
    let child_seeded = if child.board.is_empty() {
        if let Some(ms) = milestone.as_ref() {
            child.board.push(BoardItem {
                id: "local_01".into(),
                title: ms.title.clone(),
                status: ItemStatus::Pending,
                validate: ms.validate.clone(),
                ..BoardItem::default()
            });
            true
        } else if let Some(row) = parent.board.iter().find(|i| i.id == sub_task_id) {
            child.board.push(BoardItem {
                id: "local_01".into(),
                title: row.title.clone(),
                status: ItemStatus::Pending,
                validate: row.validate.clone(),
                ..BoardItem::default()
            });
            true
        } else {
            false
        }
    } else {
        false
    };
    store.save_document(child_key, child);
    log::debug!(
        "task_board gateway: dispatch_to_child child_key={child_key} sub_task_id={sub_task_id} child_seeded={child_seeded}",
    );

    let row = parent.board.iter().find(|i| i.id == sub_task_id);
    Ok(DispatchContext {
        parent_goal: parent.meta.goal.clone(),
        global_findings: parent.global_context.key_findings.clone(),
        milestone_title: row
            .map(|r| r.title.clone())
            .unwrap_or_else(|| sub_task_id.to_string()),
        milestone_validate: row.and_then(|r| r.validate.clone()),
    })
}
