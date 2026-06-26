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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode_llm::PERFORMANCE_MODE_STANDARD;
    use crate::models::{AgentModelRef, ComputerTierLlmConfig, ModelSettings};
    use std::collections::HashMap;

    #[test]
    fn resolve_prefers_computer_standard_agent_mode_llm() {
        let mut settings = ModelSettings::default();
        settings.agent_mode_llm.insert(
            "computer".into(),
            HashMap::from([(
                PERFORMANCE_MODE_STANDARD.into(),
                ComputerTierLlmConfig {
                    provider_id: "deepseek".into(),
                    model: "deepseek-v4-pro".into(),
                    enable_thinking: false,
                    thinking_budget: Some(1024),
                },
            )]),
        );
        let cfg = resolve_planner_llm(&settings, "computer");
        assert_eq!(cfg.provider_id, "deepseek");
        assert_eq!(cfg.model, "deepseek-v4-pro");
        assert!(!cfg.enable_thinking);
        assert_eq!(cfg.thinking_budget, Some(1024));
    }

    #[test]
    fn resolve_falls_back_to_computer_agent_default_model() {
        let mut settings = ModelSettings::default();
        settings.agent_default_models.insert(
            "computer".into(),
            AgentModelRef {
                provider_id: "qwen".into(),
                model: "qwen3.7-plus".into(),
            },
        );
        let cfg = resolve_planner_llm(&settings, "computer");
        assert_eq!(cfg.provider_id, "qwen");
        assert_eq!(cfg.model, "qwen3.7-plus");
        assert!(cfg.enable_thinking);
    }

    #[test]
    fn resolve_falls_back_to_active_provider_and_default_intermediate_model() {
        let mut settings = ModelSettings::default();
        settings.active_provider_id = "qwen".into();
        let cfg = resolve_planner_llm(&settings, "computer");
        assert_eq!(cfg.provider_id, "qwen");
        assert_eq!(cfg.model, DEFAULT_MODEL_INTERMEDIATE);
    }
}
