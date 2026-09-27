//! Tool-round budget tracking and exhaustion copy/compression behavior.

use crate::agent_instance_scope::AgentInstanceScope;

use super::super::session_budget::SessionToolBudget;

/// User-facing copy and compression behavior when tool budget is exhausted.
pub struct ToolBudgetExhaustionScope {
    pub error_message: String,
    pub compress_for_session: bool,
    pub compression_scope: AgentInstanceScope,
}

impl ToolBudgetExhaustionScope {
    pub(in crate::chat_service) fn lead_single(
        max_cap: u32,
        compression_scope: AgentInstanceScope,
    ) -> Self {
        let locale = crate::i18n::current_ui_locale();
        Self {
            compression_scope,
            error_message: crate::i18n::tf(
                locale,
                "errors.leadToolBudget",
                &[("max", &max_cap.to_string())],
            ),
            compress_for_session: true,
        }
    }

    pub(in crate::chat_service) fn sub_agent(
        max_cap: u32,
        compression_scope: AgentInstanceScope,
    ) -> Self {
        let locale = crate::i18n::current_ui_locale();
        Self {
            compression_scope,
            error_message: crate::i18n::tf(
                locale,
                "errors.subAgentInnerToolBudget",
                &[("max", &max_cap.to_string())],
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
