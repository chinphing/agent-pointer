//! Read-only parent board snapshot for child agents.

use crate::task_board::model::BoardDocument;
use crate::task_board::snapshot::format_parent_tunnel_block;

pub fn parent_tunnel_block(parent_doc: &BoardDocument, sub_task_id: &str) -> Option<String> {
    if parent_doc.board_is_empty() && parent_doc.meta.goal.is_empty() {
        return None;
    }
    Some(format_parent_tunnel_block(parent_doc, sub_task_id))
}
