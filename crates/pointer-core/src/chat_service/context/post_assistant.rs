//! Context for post-stream assistant turn decisions and tool-budget checks.

use super::budget::ToolBudgetRefs;
use super::llm::LlmRoundRefs;
use super::session::SessionRefs;
use super::transcript::TranscriptRefs;

/// Shared inputs for `decide_when_no_tool_calls`, `bail_on_tool_budget_exhausted`,
/// `resolve_post_assistant_action`, and `finish_tool_round_cycle`.
pub struct PostAssistantContext<'a> {
    pub session: SessionRefs<'a>,
    pub transcript: TranscriptRefs<'a>,
    pub llm: LlmRoundRefs<'a>,
    pub budget: ToolBudgetRefs<'a>,
}
