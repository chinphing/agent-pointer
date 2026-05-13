//! Built-in [`BeforeMainLlmCallHook`](super::BeforeMainLlmCallHook): append `[TASK_BOARD]` after tool/XML system blocks.

use super::{BeforeMainLlmCallContext, BeforeMainLlmCallHook};
use anyhow::Result;
use async_trait::async_trait;

pub struct TaskBoardSnapshotHook;

#[async_trait]
impl BeforeMainLlmCallHook for TaskBoardSnapshotHook {
    fn override_key(&self) -> &'static str {
        "task_board_snapshot"
    }

    fn sort_key(&self) -> &'static str {
        "_90_task_board_snapshot"
    }

    async fn execute(&self, ctx: &mut BeforeMainLlmCallContext<'_>) -> Result<()> {
        if let Some(block) = ctx
            .task_board_store
            .snapshot_for_prompt(ctx.task_board_store_key)
        {
            ctx.system_prompts.push(block);
        }
        Ok(())
    }
}
