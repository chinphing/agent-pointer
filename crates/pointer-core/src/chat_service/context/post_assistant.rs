//! Context for post-stream assistant turn decisions and tool-budget checks.

use super::budget::{ToolBudgetExhaustionScope, ToolBudgetRefs};
use super::llm::LlmRoundRefs;
use super::session::SessionRefs;
use super::transcript::TranscriptRefs;
use super::super::app_state::AppState;
use super::super::session_budget::SessionToolBudget;
use super::super::StreamTx;
use crate::models::{ChatMessage, ModelSettings};
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

/// Shared inputs for `decide_when_no_tool_calls`, `bail_on_tool_budget_exhausted`,
/// `resolve_post_assistant_action`, and `finish_tool_round_cycle`.
pub struct PostAssistantContext<'a> {
    pub session: SessionRefs<'a>,
    pub transcript: TranscriptRefs<'a>,
    pub llm: LlmRoundRefs<'a>,
    pub budget: ToolBudgetRefs<'a>,
}

impl<'a> PostAssistantContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stream: &'a StreamTx,
        state: &'a AppState,
        conversation_id: &'a str,
        cancel: &'a CancellationToken,
        history: &'a mut Vec<ChatMessage>,
        provider: &'a OpenAIProvider,
        settings: &'a ModelSettings,
        tool_budget: &'a mut SessionToolBudget,
        consumed_single: Option<&'a mut u32>,
        max_cap: u32,
        budget_scope: &'a ToolBudgetExhaustionScope,
    ) -> Self {
        Self {
            session: SessionRefs {
                stream,
                state,
                conversation_id,
                cancel,
            },
            transcript: TranscriptRefs { history },
            llm: LlmRoundRefs {
                provider,
                settings,
            },
            budget: ToolBudgetRefs {
                tool_budget,
                consumed_single,
                max_cap,
                budget_scope,
            },
        }
    }
}
