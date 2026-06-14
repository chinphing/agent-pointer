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

// Re-exports for `super::context::Type` paths across chat_service.
#[allow(unused_imports)]
pub use budget::ToolBudgetExhaustionScope;
#[allow(unused_imports)]
pub use chat_run::{ChatRunContext, ChatRunRequest};
#[allow(unused_imports)]
pub use loop_ctx::{
    LeadAgentLoopContext, SubagentDelegationContext, SubAgentLoopContext, SupervisorLoopContext,
};
#[allow(unused_imports)]
pub use post_assistant::PostAssistantContext;
#[allow(unused_imports)]
pub use prompt::{SingleAgentPromptContext, SubAgentPromptContext};
#[allow(unused_imports)]
pub use session::{SessionRefs, SessionRefsArc};
#[allow(unused_imports)]
pub use stream_round::{
    cancel_owned, LeadStreamRoundContext, StreamRoundInput, SubStreamRoundContext,
    SubStreamRoundRefs,
};
#[allow(unused_imports)]
pub use transcript::{TranscriptPersist, TranscriptRefs};
