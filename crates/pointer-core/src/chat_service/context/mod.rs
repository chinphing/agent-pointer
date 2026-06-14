//! Shared context structs for agent orchestration (P4 parameter reduction).
//!
//! Layering (bottom → top):
//! `SessionRefs` / `ToolBudgetRefs` / `LlmRoundRefs` / `TranscriptRefs`
//! → phase contexts (`PostAssistantContext`, `StreamRoundInput`, `ToolPassContext`)
//! → loop contexts (`LeadAgentLoopContext`, `SubAgentLoopContext`, …)
//! → `ChatRunContext`.

mod budget;
mod chat_run;
mod llm;
mod loop_ctx;
mod post_assistant;
mod prompt;
mod session;
mod stream_round;
mod transcript;

pub use budget::{ToolBudgetExhaustionScope, ToolBudgetRefs};
pub use chat_run::{ChatRunContext, ChatRunRequest};
pub use llm::{ConversationLlmRefs, LeadLlmSession, LlmRoundRefs};
pub use loop_ctx::{
    LeadAgentLoopContext, SubagentDelegationContext, SubAgentLoopContext, SubRunRefs,
    SupervisorLoopContext,
};
pub use post_assistant::PostAssistantContext;
pub use prompt::{PromptSessionArc, SingleAgentPromptContext, SubAgentPromptContext};
pub use session::{SessionRefs, SessionRefsArc};
pub use stream_round::{
    cancel_owned, LeadStreamRoundContext, StreamRoundInput, SubStreamRoundContext,
    SubStreamRoundRefs,
};
pub use transcript::{TranscriptPersist, TranscriptRefs};
