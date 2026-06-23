//! Planner LLM routing (Computer intermediate / standard tier).

use crate::agents::computer::tier::{
    DEFAULT_MODEL_INTERMEDIATE, PRIMARY_INTERMEDIATE_THINKING_BUDGET,
};
use crate::chat_service::resolve_provider_api_key;
use crate::mode_llm::{normalize_performance_mode, PERFORMANCE_MODE_STANDARD};
use crate::models::{ComputerTierLlmConfig, ModelSettings};
use crate::provider::OpenAIProvider;

const PLANNER_TIER_KEY: &str = "intermediate";

pub fn resolve_planner_llm(settings: &ModelSettings, _lead_agent_id: &str) -> ComputerTierLlmConfig {
    if let Some(inner) = settings.agent_mode_llm.get("computer") {
        if let Some(cfg) = inner.get(PERFORMANCE_MODE_STANDARD).filter(|c| !c.model.trim().is_empty())
        {
            return cfg.clone();
        }
        if let Some(cfg) = inner
            .get(normalize_performance_mode(PERFORMANCE_MODE_STANDARD))
            .filter(|c| !c.model.trim().is_empty())
        {
            return cfg.clone();
        }
    }
    if let Some(pref) = settings.agent_default_models.get("computer") {
        return ComputerTierLlmConfig {
            provider_id: pref.provider_id.clone(),
            model: pref.model.clone(),
            enable_thinking: true,
            thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
        };
    }
    ComputerTierLlmConfig {
        provider_id: settings.active_provider_id.clone(),
        model: DEFAULT_MODEL_INTERMEDIATE.to_string(),
        enable_thinking: true,
        thinking_budget: Some(PRIMARY_INTERMEDIATE_THINKING_BUDGET),
    }
}

pub fn planner_provider(parent: &OpenAIProvider, lead_agent_id: &str) -> OpenAIProvider {
    let cfg = resolve_planner_llm(&parent.settings, lead_agent_id);
    let mut settings = parent.settings.clone();
    if !cfg.provider_id.trim().is_empty() {
        settings.active_provider_id = cfg.provider_id.trim().to_string();
    }
    if !cfg.model.trim().is_empty() {
        settings.model = cfg.model.trim().to_string();
    }
    settings.round_enable_thinking = Some(cfg.enable_thinking);
    settings.round_thinking_budget = cfg.thinking_budget;
    let provider_switched =
        settings.active_provider_id.trim() != parent.settings.active_provider_id.trim();
    let api_key = if provider_switched {
        resolve_provider_api_key(&settings, "")
    } else {
        parent.api_key.clone()
    };
    log::info!(
        "task_board_obs: planner_model agent={} provider={} model={} tier=standard({})",
        lead_agent_id.trim(),
        settings.active_provider_id,
        settings.model,
        PLANNER_TIER_KEY
    );
    OpenAIProvider::new(settings, api_key)
}
