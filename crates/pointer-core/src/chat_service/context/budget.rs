//! Tool-round budget tracking and exhaustion copy/compression behavior.

use crate::agent_instance_scope::AgentInstanceScope;

use super::super::session_budget::SessionToolBudget;

/// User-facing copy and compression behavior when tool budget is exhausted.
pub struct ToolBudgetExhaustionScope {
    pub user_hint: String,
    pub error_message: String,
    pub compress_for_session: bool,
    pub compression_scope: AgentInstanceScope,
}

impl ToolBudgetExhaustionScope {
    pub(in crate::chat_service) fn lead_single(max_cap: u32, compression_scope: AgentInstanceScope) -> Self {
        Self {
            compression_scope,
            user_hint: format!(
                "单智能体模式下工具调用累计已达上限（{} 轮，含此前消息）。建议新开对话；将尝试压缩上下文以便查看摘要。",
                max_cap
            ),
            error_message: format!(
                "单智能体模式下工具调用轮次已达上限（{max_cap}）。请新开对话或在设置中调高上限。"
            ),
            compress_for_session: true,
        }
    }

    pub(in crate::chat_service) fn sub_agent(max_cap: u32, compression_scope: AgentInstanceScope) -> Self {
        Self {
            compression_scope,
            user_hint: format!(
                "子 Agent 内工具调用累计已达上限（{} 轮）。建议新开对话。",
                max_cap
            ),
            error_message: format!(
                "子 Agent 内工具调用轮次已达上限（{max_cap}）。请新开对话。"
            ),
            compress_for_session: false,
        }
    }
}

/// Mutable tool budget counters for one agent loop.
pub struct ToolBudgetRefs<'a> {
    pub tool_budget: &'a mut SessionToolBudget,
    pub consumed_single: Option<&'a mut u32>,
    pub max_cap: u32,
    pub budget_scope: &'a ToolBudgetExhaustionScope,
}
