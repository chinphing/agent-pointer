//! Parent / child store key ACL.

use crate::task_board::model::{BoardDocument, BoardScope};
use anyhow::{anyhow, Result};

pub const SUB_AGENT_KEY_SEP: &str = "\u{1f}ptr_sub_agent\u{1f}";

pub fn sub_agent_task_board_store_key(parent_store_key: &str, supervisor_task_id: &str) -> String {
    format!(
        "{parent}{SUB_AGENT_KEY_SEP}{task}",
        parent = parent_store_key.trim(),
        task = supervisor_task_id.trim()
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

/// Child boards may not replace/init parent milestones via mistaken scope; host binds `_conversation_id` to child key only.
pub fn assert_child_may_mutate(store_key: &str, doc: &BoardDocument, method: &str) -> Result<()> {
    if !is_child_store_key(store_key) {
        return Ok(());
    }
    let m = method.trim().to_ascii_lowercase();
    if m == "sync_finding" || m == "check_deps" {
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

    #[test]
    fn parent_key_from_child() {
        let k = sub_agent_task_board_store_key("conv-1\u{1f}ptr_main_turn\u{1f}msg-1", "task_a");
        assert_eq!(
            parent_store_key_from_child(&k).as_deref(),
            Some("conv-1\u{1f}ptr_main_turn\u{1f}msg-1")
        );
    }
}
