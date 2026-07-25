//! Legacy dynamic `[TASK_BOARD]` system inject path used for rollback compatibility.

use super::{BeforeMainLlmCallContext, BeforeMainLlmCallHook};
use crate::task_board::sub_agent_hint::task_board_init_hint;
use anyhow::Result;
use async_trait::async_trait;

pub struct TaskBoardSnapshotHook;

pub fn append_task_board_dynamic_block(
    system_prompts_dynamic: &mut Vec<String>,
    task_board_store: &crate::task_board::TaskBoardStore,
    task_board_store_key: &str,
    conversation_id: &str,
    lead_agent_profile: &crate::agents::AgentProfile,
) {
    let doc = task_board_store.document(task_board_store_key);
    if matches!(
        doc.meta.status,
        crate::task_board::MetaStatus::Completed | crate::task_board::MetaStatus::Failed
    ) {
        return;
    }
    if let Some(block) = task_board_store.snapshot_for_prompt(task_board_store_key) {
        system_prompts_dynamic.push(block);
        return;
    }
    if let Some(hint) =
        task_board_init_hint(task_board_store, task_board_store_key, lead_agent_profile)
    {
        crate::task_board::observability::log_main_agent_init_hint(
            conversation_id,
            task_board_store_key,
            lead_agent_profile,
        );
        system_prompts_dynamic.push(hint);
    }
}

#[async_trait]
impl BeforeMainLlmCallHook for TaskBoardSnapshotHook {
    fn override_key(&self) -> &'static str {
        "task_board_snapshot"
    }

    fn sort_key(&self) -> &'static str {
        "_90_task_board_snapshot"
    }

    async fn execute(&self, ctx: &mut BeforeMainLlmCallContext<'_>) -> Result<()> {
        append_task_board_dynamic_block(
            ctx.system_prompts_dynamic,
            ctx.task_board_store.as_ref(),
            ctx.task_board_store_key,
            ctx.conversation_id,
            &ctx.lead_agent_profile,
        );
        Ok(())
    }
}
