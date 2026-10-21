//! `locate_element` async dispatch — B5 visual grounding.
//!
//! Runs on the chat runtime async path (like `web_search`) because it needs the
//! conversation's [`ComputerState`] for a fresh screenshot plus the agent's
//! resolved provider for the vision calls.

use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;

pub(super) async fn dispatch_locate_element(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let _ = tc;
    let payload = crate::agents::computer::locate::run_locate_element(
        state.computer_state.as_ref(),
        conversation_id,
        provider,
        &args_value,
        cancel,
    )
    .await?;
    Ok((payload, true, None))
}
