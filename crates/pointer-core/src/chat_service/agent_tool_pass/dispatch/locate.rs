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
    // Grounding can run on its own model — a cheap vision model is usually
    // enough and keeps the agent's primary model free. Empty config falls back
    // to the agent's model (apply_pipeline_phase_settings leaves `model` alone
    // only when the override is non-empty... it always sets it, so we gate).
    let phase_settings = {
        let cfg = state.computer_state.effective_tier_config();
        let model = cfg.pipeline_llm.model_locate.trim();
        if model.is_empty() {
            provider.settings.clone()
        } else {
            state.computer_state.apply_pipeline_phase_settings(
                conversation_id,
                crate::agents::computer::tier::PipelineLlmPhase::Locate,
                &provider.settings,
            )
        }
    };
    let locate_provider = OpenAIProvider::new(phase_settings, provider.api_key.clone());
    let payload = crate::agents::computer::locate::run_locate_element(
        state.computer_state.as_ref(),
        conversation_id,
        &locate_provider,
        &args_value,
        cancel,
    )
    .await?;
    Ok((payload, true, None))
}
