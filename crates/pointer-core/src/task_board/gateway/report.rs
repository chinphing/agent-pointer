//! Child completion settlement on parent board.

use super::super::model::{compact_snippet, BoardDocument, ItemStatus};
use super::dependency::mark_ready_after_report;
use anyhow::{anyhow, Result};

pub fn report_child_status(
    parent: &mut BoardDocument,
    sub_task_id: &str,
    status: ItemStatus,
    output: &str,
) -> Result<()> {
    let idx = parent
        .global_milestones
        .iter()
        .position(|i| i.id == sub_task_id)
        .ok_or_else(|| anyhow!("parent board missing milestone {sub_task_id}"))?;
    let row = &mut parent.global_milestones[idx];
    row.status = status;
    if !output.trim().is_empty() {
        row.remark = Some(compact_snippet(output.trim()));
    }
    mark_ready_after_report(parent);
    log::info!(
        "task_board gateway: report_child_status id={sub_task_id} status={}",
        status.as_str()
    );
    Ok(())
}
