//! `before_main_llm_call` — inject `[LOCKED GOAL]` into system dynamic slice.

use crate::agents::AgentProfile;
use crate::extensions::{BeforeMainLlmCallContext, BeforeMainLlmCallHook, ExtensionRegistry};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub fn register(registry: &mut ExtensionRegistry) {
    registry.register_before_main_llm_call(Arc::new(ComputerTierDynamicHook));
}

struct ComputerTierDynamicHook;

#[async_trait]
impl BeforeMainLlmCallHook for ComputerTierDynamicHook {
    fn override_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("_15_computer_tier_dynamic")
    }

    fn sort_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("_15_computer_tier_dynamic")
    }

    async fn execute(&self, ctx: &mut BeforeMainLlmCallContext<'_>) -> Result<()> {
        if ctx.lead_agent_profile != AgentProfile::Computer {
            return Ok(());
        }
        if let Some(block) = ctx
            .computer_state
            .locked_goal_dynamic_block(ctx.conversation_id)
        {
            ctx.system_prompts_dynamic.push(block);
        }
        Ok(())
    }
}
