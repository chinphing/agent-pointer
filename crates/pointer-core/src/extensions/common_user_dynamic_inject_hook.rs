//! Common `message_loop_prompts_after` hook: append runtime task board markdown as the last user message.

use crate::agents::AgentProfile;
use crate::extensions::{
    new_extension_message_id, now_ms, MessageLoopPromptsAfterContext, MessageLoopPromptsAfterHook,
};
use crate::models::{ChatMessage, Role};
use crate::task_board::MetaStatus;
use crate::task_board::snapshot::markdown_runtime_block_for_inject;
use crate::task_board::sub_agent_hint::main_agent_task_board_init_hint;
use anyhow::Result;
use async_trait::async_trait;

pub struct CommonUserDynamicInjectHook;

#[async_trait]
impl MessageLoopPromptsAfterHook for CommonUserDynamicInjectHook {
    fn override_key(&self) -> &'static str {
        "_99_common_user_dynamic_inject"
    }

    fn sort_key(&self) -> &'static str {
        "_99_common_user_dynamic_inject"
    }

    async fn execute(&self, ctx: &mut MessageLoopPromptsAfterContext<'_>) -> Result<()> {
        if !ctx.user_dynamic_inject_enabled {
            return Ok(());
        }

        let doc = ctx.task_board_store.document(ctx.task_board_store_key);
        let terminal_board = matches!(doc.meta.status, MetaStatus::Completed | MetaStatus::Failed);
        let has_board_content =
            !terminal_board && (!doc.meta.goal.trim().is_empty() || !doc.board.is_empty());
        let board_block = if has_board_content {
            Some(markdown_runtime_block_for_inject(&doc))
        } else if ctx.task_board_store_key == ctx.conversation_id {
            main_agent_task_board_init_hint(
                ctx.task_board_store.as_ref(),
                ctx.task_board_store_key,
                &ctx.lead_agent_profile,
            )
            .inspect(|_| {
                crate::task_board::observability::log_main_agent_init_hint(
                    ctx.conversation_id,
                    ctx.task_board_store_key,
                    &ctx.lead_agent_profile,
                );
            })
        } else {
            None
        };

        let Some(content) = board_block else {
            return Ok(());
        };

        let legacy_snapshot_len = ctx
            .task_board_store
            .snapshot_for_prompt(ctx.task_board_store_key)
            .map(|s| s.len())
            .unwrap_or(0);
        let user_block_len = content.len();

        ctx.messages.push(ChatMessage {
            id: new_extension_message_id("user_dynamic_inject"),
            role: Role::User,
            content,
            status: "done".into(),
            created_at: now_ms(),
            tool_calls: None,
            tool_call_id: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
        attachments: None,
        });
        if ctx.lead_agent_profile == AgentProfile::Computer
            || ctx.lead_agent_profile == AgentProfile::Coder
        {
            log::info!(
                "common_user_dynamic_inject: appended user inject conversation_id={} store_key={} legacy_snapshot_len={} user_block_len={}",
                ctx.conversation_id,
                ctx.task_board_store_key,
                legacy_snapshot_len,
                user_block_len
            );
        }
        Ok(())
    }
}
