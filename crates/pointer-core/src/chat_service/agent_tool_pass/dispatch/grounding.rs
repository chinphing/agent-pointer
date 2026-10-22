//! Shared provider resolution for visual grounding.
//!
//! Grounding runs on its own model by default (`pipeline_llm.model_locate`) so
//! the agent's primary model stays free, and the key has to come from whichever
//! provider that resolves to — reusing the agent's key shows up as a 401 from
//! the wrong endpoint.
//!
//! Three call sites need this: `locate_element`, the pointing tools, and
//! `input_at`. It used to be copy-pasted; the copies drifted once already.

use crate::provider::OpenAIProvider;
use crate::agents::computer::tier::PipelineLlmPhase;

use super::super::super::app_state::AppState;

/// Build the provider used for grounding calls.
///
/// Falls back to the agent's own settings when no locate model is configured.
pub(super) fn locate_provider(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
) -> anyhow::Result<OpenAIProvider> {
    let phase_settings = {
        let cfg = state.computer_state.effective_tier_config();
        let model = cfg.pipeline_llm.model_locate.trim();
        if model.is_empty() {
            provider.settings.clone()
        } else {
            state.computer_state.apply_pipeline_phase_settings(
                conversation_id,
                PipelineLlmPhase::Locate,
                &provider.settings,
            )
        }
    };
    let key = crate::provider::api_key_for_settings(&phase_settings)?;
    Ok(OpenAIProvider::new(phase_settings, key))
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_is_wired() {
        // Compilation is the assertion: the signature must stay callable from
        // locate.rs, mouse.rs and input.rs with the same arguments.
        let _ = super::locate_provider;
    }
}
