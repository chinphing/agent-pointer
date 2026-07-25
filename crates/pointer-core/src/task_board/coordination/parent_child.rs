//! Parent / child store key ACL.

use crate::task_board::model::{BoardDocument, BoardScope};
use crate::task_board::store::TaskBoardStore;
use anyhow::{anyhow, Result};

pub const SUB_AGENT_KEY_SEP: &str = "\u{1f}ptr_sub_agent\u{1f}";
pub const SUB_AGENT_INSTANCE_KEY_SEP: &str = "\u{1f}ptr_agent_instance\u{1f}";

pub fn sub_agent_task_board_store_key(parent_store_key: &str, supervisor_task_id: &str) -> String {
    format!(
        "{parent}{SUB_AGENT_KEY_SEP}{task}",
        parent = parent_store_key.trim(),
        task = supervisor_task_id.trim()
    )
}

pub fn sub_agent_task_board_store_key_for_instance(
    parent_store_key: &str,
    supervisor_task_id: &str,
    agent_instance_id: &str,
) -> String {
    format!(
        "{}{SUB_AGENT_INSTANCE_KEY_SEP}{}",
        sub_agent_task_board_store_key(parent_store_key, supervisor_task_id),
        agent_instance_id.trim()
    )
}

pub fn parent_store_key_from_child(child_store_key: &str) -> Option<String> {
    let (main, _) = child_store_key.split_once(SUB_AGENT_KEY_SEP)?;
    let main = main.trim();
    if main.is_empty() {
        None
    } else {
        Some(main.to_string())
    }
}

pub fn is_child_store_key(store_key: &str) -> bool {
    store_key.contains(SUB_AGENT_KEY_SEP)
}

fn store_key_has_board(board_store: &TaskBoardStore, store_key: &str) -> bool {
    let doc = board_store.document(store_key);
    !doc.board_is_empty() || !doc.meta.goal.trim().is_empty()
}

/// Resolve persisted store key for UI reads (snapshot).
///
/// After reload, `parent_store_key` from memory may not match the parent used at init;
/// child boards fall back to suffix search under `conversation_id` (same as snapshot API).
pub fn resolve_store_key_for_read(
    board_store: &TaskBoardStore,
    conversation_id: &str,
    task_id: Option<&str>,
    parent_store_key: &str,
) -> String {
    let conv = conversation_id.trim();
    let parent = parent_store_key.trim();
    match task_id.map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            if parent.is_empty() {
                conv.to_string()
            } else {
                parent.to_string()
            }
        }
        Some(tid) => {
            let parent_key = if parent.is_empty() { conv } else { parent };
            let preferred = sub_agent_task_board_store_key(parent_key, tid);
            if store_key_has_board(board_store, &preferred) {
                return preferred;
            }
            let suffix = format!("{SUB_AGENT_KEY_SEP}{tid}");
            let matches: Vec<String> = board_store
                .list_store_keys_by_prefix(conv)
                .into_iter()
                .filter(|k| k.ends_with(&suffix))
                .filter(|k| store_key_has_board(board_store, k))
                .collect();
            matches.into_iter().next().unwrap_or(preferred)
        }
    }
}

/// Child boards may not replace/init parent milestones via mistaken scope; host binds `_conversation_id` to child key only.
pub fn assert_child_may_mutate(store_key: &str, doc: &BoardDocument, method: &str) -> Result<()> {
    if !is_child_store_key(store_key) {
        return Ok(());
    }
    let m = method.trim().to_ascii_lowercase();
    if m == "check_deps" {
        return Ok(());
    }
    if doc.meta.scope == Some(BoardScope::Child) || is_child_store_key(store_key) {
        return Ok(());
    }
    Err(anyhow!("task_board: child scope required for store key"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_board::model::{BoardDocument, MetaStatus};
    use crate::task_board::store::TaskBoardStore;

    #[test]
    fn parent_key_from_child() {
        let k = sub_agent_task_board_store_key("conv-1\u{1f}ptr_main_turn\u{1f}msg-1", "task_a");
        assert_eq!(
            parent_store_key_from_child(&k).as_deref(),
            Some("conv-1\u{1f}ptr_main_turn\u{1f}msg-1")
        );
    }

    #[test]
    fn same_user_task_id_has_unique_child_key_per_instance() {
        let parent = "conv-1\u{1f}ptr_main_turn\u{1f}msg-1";
        let first =
            sub_agent_task_board_store_key_for_instance(parent, "shared-task", "fork-instance-a");
        let second =
            sub_agent_task_board_store_key_for_instance(parent, "shared-task", "fork-instance-b");

        assert_ne!(first, second);
        assert_eq!(parent_store_key_from_child(&first).as_deref(), Some(parent));
        assert_eq!(
            parent_store_key_from_child(&second).as_deref(),
            Some(parent)
        );
    }

    #[test]
    fn resolve_child_store_key_falls_back_when_parent_mismatch() {
        let board_store = TaskBoardStore::new();
        let conv = "conv-1";
        let real_parent = format!("{conv}\u{1f}ptr_main_turn\u{1f}msg-1");
        let task_id = "task_a";
        let real_child = sub_agent_task_board_store_key(&real_parent, task_id);
        let mut doc = BoardDocument::empty_for_store_key(&real_child);
        doc.meta.goal = "child goal".into();
        doc.meta.status = MetaStatus::Running;
        board_store.save_document(&real_child, doc);

        let wrong_parent = conv;
        let preferred = sub_agent_task_board_store_key(wrong_parent, task_id);
        assert!(!store_key_has_board(&board_store, &preferred));

        let resolved = resolve_store_key_for_read(&board_store, conv, Some(task_id), wrong_parent);
        assert_eq!(resolved, real_child);
    }
}
