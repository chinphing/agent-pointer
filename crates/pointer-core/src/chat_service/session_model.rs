use crate::agents::DEFAULT_LEAD_AGENT_ID;
use crate::mode_llm::resolve_agent_mode_llm_config;
use crate::models::ModelSettings;
use crate::provider::OpenAIProvider;
use crate::thinking_strategy::apply_tier_thinking_to_round;

/// Apply per-agent default model from `agent_default_models` when `agent_id` has an entry.
/// Returns true when provider and/or model were overridden.
pub(crate) fn apply_agent_model_defaults(settings: &mut ModelSettings, agent_id: &str) -> bool {
    let key = agent_id.trim();
    if key.is_empty() {
        return false;
    }
    if let Some(cfg) = resolve_agent_mode_llm_config(settings, key).cloned() {
        let provider_id = cfg.provider_id.trim().to_string();
        let model = cfg.model.trim().to_string();
        let mut applied = false;
        if !provider_id.is_empty() {
            settings.active_provider_id = provider_id;
            applied = true;
        }
        if !model.is_empty() {
            settings.model = model;
            applied = true;
        }
        if applied {
            apply_tier_thinking_to_round(settings, &cfg);
            return true;
        }
    }
    if let Some(pref) = settings.agent_default_models.get(key).cloned() {
        let mut applied = false;
        if !pref.provider_id.trim().is_empty() {
            settings.active_provider_id = pref.provider_id.trim().to_string();
            applied = true;
        }
        if !pref.model.trim().is_empty() {
            settings.model = pref.model.trim().to_string();
            applied = true;
        }
        return applied;
    }
    false
}

fn provider_has_api_key(settings: &ModelSettings, provider_id: &str) -> bool {
    let pid = provider_id.trim();
    if pid.is_empty() {
        return false;
    }
    settings
        .providers
        .iter()
        .any(|p| p.id == pid && !p.api_key.trim().is_empty())
}

fn provider_model_in_list(settings: &ModelSettings, provider_id: &str, model: &str) -> bool {
    let pid = provider_id.trim();
    let model = model.trim();
    if pid.is_empty() || model.is_empty() {
        return false;
    }
    settings
        .providers
        .iter()
        .any(|p| p.id == pid && p.models.iter().any(|m| m.trim() == model))
}

/// Usable when the provider has a key, and either has an empty catalog (no
/// restriction) or lists the requested model.
fn provider_model_usable(settings: &ModelSettings, provider_id: &str, model: &str) -> bool {
    let pid = provider_id.trim();
    let model = model.trim();
    if pid.is_empty() || model.is_empty() || !provider_has_api_key(settings, pid) {
        return false;
    }
    let Some(provider) = settings.providers.iter().find(|p| p.id == pid) else {
        return false;
    };
    if provider.models.is_empty() {
        return true;
    }
    provider.models.iter().any(|m| m.trim() == model)
}

/// Prefer `preferred` when it is in the provider list; otherwise first listed model.
fn resolve_fallback_model(settings: &ModelSettings, provider_id: &str, preferred: &str) -> String {
    let preferred = preferred.trim();
    if provider_model_in_list(settings, provider_id, preferred) {
        return preferred.to_string();
    }
    settings
        .providers
        .iter()
        .find(|p| p.id == provider_id.trim())
        .and_then(|p| {
            p.models
                .iter()
                .map(|m| m.trim())
                .find(|m| !m.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| preferred.to_string())
}

/// If the mode/agent override landed on an unusable provider/model (no API key,
/// or model missing from a non-empty catalog), restore the previously active
/// provider + a model that provider actually lists.
fn fallback_to_active_if_unusable(
    settings: &mut ModelSettings,
    prior_provider_id: &str,
    prior_model: &str,
    context: &str,
) {
    let resolved_pid = settings.active_provider_id.trim().to_string();
    let resolved_model = settings.model.trim().to_string();
    if provider_model_usable(settings, &resolved_pid, &resolved_model) {
        return;
    }
    let prior_pid = prior_provider_id.trim();
    if prior_pid.is_empty() || !provider_has_api_key(settings, prior_pid) {
        return;
    }
    let fallback_model = resolve_fallback_model(settings, prior_pid, prior_model);
    if resolved_pid == prior_pid && resolved_model == fallback_model.trim() {
        return;
    }
    let reason = if !provider_has_api_key(settings, &resolved_pid) {
        "has no API key"
    } else {
        "model not in provider catalog"
    };
    log::warn!(
        "llm: mode/agent model provider={resolved_pid} model={resolved_model} {reason}; \
         falling back to active provider={prior_pid} model={} ({context})",
        fallback_model.trim()
    );
    settings.active_provider_id = prior_pid.to_string();
    if !fallback_model.trim().is_empty() {
        settings.model = fallback_model;
    }
}

fn resolve_lead_agent_key(
    settings: &ModelSettings,
    _effective_agent_mode: &str,
    lead_agent_id_override: Option<&str>,
) -> String {
    if let Some(id) = lead_agent_id_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return id.to_string();
    }
    let id = settings.lead_agent_id.trim();
    if id.is_empty() {
        DEFAULT_LEAD_AGENT_ID.to_string()
    } else {
        id.to_string()
    }
}

pub(crate) fn apply_session_agent_model_defaults(
    settings: &mut ModelSettings,
    effective_agent_mode: &str,
    lead_agent_id_override: Option<&str>,
    performance_mode_override: Option<&str>,
) {
    let prior_provider = settings.active_provider_id.clone();
    let prior_model = settings.model.clone();
    let key = resolve_lead_agent_key(settings, effective_agent_mode, lead_agent_id_override);
    if let Some(mode) = performance_mode_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        settings
            .agent_performance_modes
            .insert(key.clone(), mode.to_string());
    }
    let _ = apply_agent_model_defaults(settings, &key);
    fallback_to_active_if_unusable(settings, &prior_provider, &prior_model, "session");
}

/// Resolve API key for `settings.active_provider_id`.
/// When the active provider entry exists but has no key, returns empty (do not borrow another provider's key).
pub(crate) fn resolve_provider_api_key(settings: &ModelSettings, fallback_api_key: &str) -> String {
    let pid = settings.active_provider_id.trim();
    if let Some(p) = settings.providers.iter().find(|p| p.id == pid) {
        if !p.api_key.trim().is_empty() {
            return p.api_key.clone();
        }
        return String::new();
    }
    fallback_api_key.trim().to_string()
}

/// Apply per-agent model defaults and attach the matching provider API key for the active provider.
pub(crate) fn prepare_session_llm_settings(
    settings: &mut ModelSettings,
    effective_agent_mode: &str,
    lead_agent_id_override: Option<&str>,
    performance_mode_override: Option<&str>,
) -> String {
    let fallback_key = settings.api_key.clone();
    apply_session_agent_model_defaults(
        settings,
        effective_agent_mode,
        lead_agent_id_override,
        performance_mode_override,
    );
    let api_key = resolve_provider_api_key(settings, &fallback_key);
    settings.api_key = api_key.clone();
    settings.has_key = !api_key.is_empty();
    api_key
}

/// Build a provider for a sub-agent run, honoring per-agent `agentDefaultModels` when set.
pub(crate) fn sub_agent_provider(parent: &OpenAIProvider, sub_agent_id: &str) -> OpenAIProvider {
    let mut settings = parent.settings.clone();
    let prior_provider = settings.active_provider_id.clone();
    let prior_model = settings.model.clone();
    let applied = apply_agent_model_defaults(&mut settings, sub_agent_id);
    if !applied {
        return OpenAIProvider::new(settings, parent.api_key.clone());
    }
    fallback_to_active_if_unusable(
        &mut settings,
        &prior_provider,
        &prior_model,
        &format!("sub_agent={}", sub_agent_id.trim()),
    );
    let provider_switched =
        settings.active_provider_id.trim() != parent.settings.active_provider_id.trim();
    let api_key = if provider_switched {
        resolve_provider_api_key(&settings, "")
    } else {
        parent.api_key.clone()
    };
    if crate::logging::internal_runtime_log_enabled() {
        log::debug!(
            "sub_agent model override agent_id={} provider={} model={} provider_switched={}",
            sub_agent_id.trim(),
            settings.active_provider_id,
            settings.model,
            provider_switched
        );
    }
    OpenAIProvider::new(settings, api_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentModelRef, ComputerTierLlmConfig, ProviderConfig};

    fn sample_settings() -> ModelSettings {
        ModelSettings {
            providers: vec![
                ProviderConfig {
                    id: "qwen".into(),
                    name: "Qwen".into(),
                    base_url: "https://example.com".into(),
                    api_key: "qwen-key".into(),
                    models: vec!["qwen-plus".into(), "qwen-turbo".into()],
                    reasoning_in_messages: None,
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    context_budget_tokens: None,
                    model_configs: Default::default(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                    thinking_protocol: None,
                    thinking_intensity: None,
                    extra_body: None,
                    source: None,
                },
                ProviderConfig {
                    id: "openai".into(),
                    name: "OpenAI".into(),
                    base_url: "https://api.openai.com".into(),
                    api_key: "openai-key".into(),
                    models: vec!["gpt-4o".into()],
                    reasoning_in_messages: None,
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    context_budget_tokens: None,
                    model_configs: Default::default(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                    thinking_protocol: None,
                    thinking_intensity: None,
                    extra_body: None,
                    source: None,
                },
            ],
            active_provider_id: "qwen".into(),
            model: "qwen-plus".into(),
            agent_default_models: [(
                "explore".into(),
                AgentModelRef {
                    provider_id: "openai".into(),
                    model: "gpt-4o".into(),
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn apply_agent_model_defaults_overrides_provider_and_model() {
        let mut settings = sample_settings();
        assert!(apply_agent_model_defaults(&mut settings, "explore"));
        assert_eq!(settings.active_provider_id, "openai");
        assert_eq!(settings.model, "gpt-4o");
    }

    #[test]
    fn apply_agent_model_defaults_noop_for_unknown_agent() {
        let mut settings = sample_settings();
        assert!(!apply_agent_model_defaults(&mut settings, "computer"));
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.model, "qwen-plus");
    }

    #[test]
    fn apply_agent_model_defaults_applies_mode_llm_for_coder() {
        let mut settings = sample_settings();
        // 平台下发档位默认后（agentModeLlm），coder fast → deepseek-v4-flash。
        settings.agent_mode_llm.insert(
            "coder".into(),
            [(
                "fast".into(),
                ComputerTierLlmConfig {
                    provider_id: "deepseek".into(),
                    model: "deepseek-v4-flash".into(),
                    enable_thinking: true,
                    thinking_budget: Some(2048),
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );
        assert!(apply_agent_model_defaults(&mut settings, "coder"));
        assert_eq!(settings.active_provider_id, "deepseek");
        assert_eq!(settings.model, "deepseek-v4-flash");
        assert!(settings.round_thinking_locked);
        assert_eq!(settings.round_thinking_intensity, None);
        assert_eq!(settings.round_enable_thinking, None);
    }

    #[test]
    fn prepare_session_falls_back_to_active_when_mode_provider_has_no_key() {
        let mut settings = sample_settings();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        // coder fast → deepseek (not in providers / no key); active qwen has key.
        let key = prepare_session_llm_settings(&mut settings, "single", None, None);
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.model, "qwen-plus");
        assert_eq!(key, "qwen-key");
    }

    #[test]
    fn prepare_session_fallback_picks_listed_model_when_prior_not_in_catalog() {
        let mut settings = sample_settings();
        // Stale built-in default is not in the standalone-replaced catalog.
        settings.model = "qwen3.5-plus".into();
        settings.providers[0].models = vec!["qwen3.6-27b".into()];
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        let key = prepare_session_llm_settings(&mut settings, "single", None, None);
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.model, "qwen3.6-27b");
        assert_eq!(key, "qwen-key");
    }

    #[test]
    fn prepare_session_fallback_when_mode_model_missing_from_keyed_catalog() {
        let mut settings = sample_settings();
        // expert → qwen/qwen3.7-plus via agentModeLlm, but catalog only has local id.
        settings
            .agent_performance_modes
            .insert("coder".into(), "expert".into());
        settings.providers[0].models = vec!["qwen3.6-27b".into()];
        settings.model = "qwen3.6-27b".into();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        let key = prepare_session_llm_settings(&mut settings, "single", None, None);
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.model, "qwen3.6-27b");
        assert_eq!(key, "qwen-key");
    }

    #[test]
    fn prepare_session_keeps_mode_model_when_provider_has_key() {
        let mut settings = sample_settings();
        settings.agent_mode_llm.insert(
            "coder".into(),
            [(
                "fast".into(),
                ComputerTierLlmConfig {
                    provider_id: "deepseek".into(),
                    model: "deepseek-v4-flash".into(),
                    enable_thinking: true,
                    thinking_budget: Some(2048),
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );
        settings.providers.push(ProviderConfig {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            base_url: "https://api.deepseek.com".into(),
            api_key: "ds-key".into(),
            models: vec!["deepseek-v4-flash".into()],
            reasoning_in_messages: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            context_budget_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: None,
        });
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        let key = prepare_session_llm_settings(&mut settings, "single", None, None);
        assert_eq!(settings.active_provider_id, "deepseek");
        assert_eq!(settings.model, "deepseek-v4-flash");
        assert_eq!(key, "ds-key");
    }

    #[test]
    fn sub_agent_provider_falls_back_when_override_provider_has_no_key() {
        let mut settings = sample_settings();
        // explore → openai; clear openai key so fallback should restore qwen.
        settings.providers[1].api_key.clear();
        let parent = OpenAIProvider::new(settings, "qwen-key".into());
        let sub = sub_agent_provider(&parent, "explore");
        assert_eq!(sub.settings.active_provider_id, "qwen");
        assert_eq!(sub.settings.model, "qwen-plus");
        assert_eq!(sub.api_key, "qwen-key");
    }

    #[test]
    fn sub_agent_provider_uses_per_agent_defaults() {
        let parent = OpenAIProvider::new(sample_settings(), "parent-key".into());
        let sub = sub_agent_provider(&parent, "explore");
        assert_eq!(sub.settings.active_provider_id, "openai");
        assert_eq!(sub.settings.model, "gpt-4o");
        assert_eq!(sub.api_key, "openai-key");
    }

    #[test]
    fn sub_agent_provider_keeps_parent_when_no_override() {
        let parent = OpenAIProvider::new(sample_settings(), "parent-key".into());
        let sub = sub_agent_provider(&parent, "computer");
        assert_eq!(sub.settings.active_provider_id, "qwen");
        assert_eq!(sub.settings.model, "qwen-plus");
        assert_eq!(sub.api_key, "parent-key");
    }

    #[test]
    fn resolve_provider_api_key_does_not_borrow_sibling_provider_key() {
        let mut settings = sample_settings();
        settings.active_provider_id = "openai".into();
        settings.providers[1].api_key.clear();
        assert!(resolve_provider_api_key(&settings, "qwen-key").is_empty());
    }

    #[test]
    fn prepare_session_llm_settings_uses_target_provider_key_after_agent_override() {
        let mut settings = sample_settings();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "explore".into();
        let key = prepare_session_llm_settings(&mut settings, "single", None, None);
        assert_eq!(settings.active_provider_id, "openai");
        assert_eq!(key, "openai-key");
        assert_ne!(key, "qwen-key");
    }

    #[test]
    fn prepare_session_llm_settings_honors_lead_agent_override() {
        let mut settings = sample_settings();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        // IM session override should win over global settings.lead_agent_id.
        let key = prepare_session_llm_settings(&mut settings, "single", Some("explore"), None);
        assert_eq!(settings.active_provider_id, "openai");
        assert_eq!(settings.model, "gpt-4o");
        assert_eq!(key, "openai-key");
    }

    #[test]
    fn prepare_session_performance_override_wins_over_global_mode() {
        let mut settings = sample_settings();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        // Global default: coder → fast. Override must inject expert for the session key.
        settings.providers[0].models = vec!["qwen-fast".into(), "qwen-expert".into()];
        settings
            .agent_performance_modes
            .insert("coder".into(), "fast".into());
        settings.agent_mode_llm.insert(
            "coder".into(),
            [
                (
                    "fast".into(),
                    ComputerTierLlmConfig {
                        provider_id: "qwen".into(),
                        model: "qwen-fast".into(),
                        enable_thinking: true,
                        thinking_budget: Some(1024),
                        reasoning_effort: None,
                        thinking_intensity: None,
                    },
                ),
                (
                    "expert".into(),
                    ComputerTierLlmConfig {
                        provider_id: "qwen".into(),
                        model: "qwen-expert".into(),
                        enable_thinking: true,
                        thinking_budget: Some(4096),
                        reasoning_effort: None,
                        thinking_intensity: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        );
        let key = prepare_session_llm_settings(&mut settings, "single", None, Some("expert"));
        assert_eq!(
            settings
                .agent_performance_modes
                .get("coder")
                .map(String::as_str),
            Some("expert")
        );
        assert_eq!(settings.model, "qwen-expert");
        assert_eq!(key, "qwen-key");
    }
}
