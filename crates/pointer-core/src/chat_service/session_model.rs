use crate::mode_llm::{
    agent_performance_mode, parse_performance_mode, resolve_agent_mode_llm_config_for_mode,
    resolve_sub_agent_performance_mode,
};
use crate::models::ModelSettings;
use crate::provider::OpenAIProvider;
use crate::thinking_strategy::apply_tier_thinking_to_round;

/// Apply the per-agent model for an explicit performance `mode`.
///
/// Order: `agentModeLlm[agent_id][mode]` → `agentDefaultModels[agent_id]` →
/// keep the current provider/model. Returns true when provider and/or model changed.
pub(crate) fn apply_agent_model_defaults_for_mode(
    settings: &mut ModelSettings,
    agent_id: &str,
    mode: &str,
) -> bool {
    let key = agent_id.trim();
    if key.is_empty() {
        return false;
    }
    if let Some(cfg) = resolve_agent_mode_llm_config_for_mode(settings, key, mode).cloned() {
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

/// Apply per-agent default model from `agent_default_models` / `agentModeLlm` when
/// `agent_id` has an entry. The mode comes from `agentPerformanceModes[agent_id]`
/// (or the global `fast` default) — the lead / global entry point.
/// Returns true when provider and/or model were overridden.
pub(crate) fn apply_agent_model_defaults(settings: &mut ModelSettings, agent_id: &str) -> bool {
    let key = agent_id.trim();
    if key.is_empty() {
        return false;
    }
    let mode = agent_performance_mode(settings, key);
    apply_agent_model_defaults_for_mode(settings, key, mode)
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

fn resolve_lead_agent_key(settings: &ModelSettings, lead_agent_id_override: Option<&str>) -> String {
    if let Some(id) = lead_agent_id_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return id.to_string();
    }
    settings.lead_agent_id.trim().to_string()
}

pub(crate) fn apply_session_agent_model_defaults(
    settings: &mut ModelSettings,
    lead_agent_id_override: Option<&str>,
    performance_mode_override: Option<&str>,
) {
    let prior_provider = settings.active_provider_id.clone();
    let prior_model = settings.model.clone();
    let key = resolve_lead_agent_key(settings, lead_agent_id_override);
    if key.is_empty() {
        fallback_to_active_if_unusable(settings, &prior_provider, &prior_model, "session");
        return;
    }
    // The session picker override applies to the lead only. It must **not** be
    // written into `agentPerformanceModes`: that map is the local per-agent
    // default and is also read by same-id sub-agents (self forks), so mutating it
    // leaked the session tier onto children.
    match performance_mode_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(parse_performance_mode)
    {
        Some(mode) => {
            apply_agent_model_defaults_for_mode(settings, &key, mode);
        }
        None => {
            apply_agent_model_defaults(settings, &key);
        }
    }
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
    lead_agent_id_override: Option<&str>,
    performance_mode_override: Option<&str>,
) -> String {
    let fallback_key = settings.api_key.clone();
    apply_session_agent_model_defaults(settings, lead_agent_id_override, performance_mode_override);
    let api_key = resolve_provider_api_key(settings, &fallback_key);
    settings.api_key = api_key.clone();
    settings.has_key = !api_key.is_empty();
    api_key
}

/// Build a provider for a sub-agent run, honoring the sub-agent performance-mode
/// chain (`performanceMode` > local `agentPerformanceModes[agentId]` > `standard`)
/// and per-agent `agentDefaultModels` when set.
pub(crate) fn sub_agent_provider(
    parent: &OpenAIProvider,
    sub_agent_id: &str,
    performance_mode: Option<&str>,
) -> OpenAIProvider {
    sub_agent_provider_with_workspace(parent, sub_agent_id, performance_mode, None)
}

/// [`sub_agent_provider`] variant that replaces the inherited workspace first
/// (self fork: the snapshot's workspace wins).
pub(crate) fn sub_agent_provider_with_workspace(
    parent: &OpenAIProvider,
    sub_agent_id: &str,
    performance_mode: Option<&str>,
    workspace_root_override: Option<&str>,
) -> OpenAIProvider {
    let mut settings = parent.settings.clone();
    if let Some(root) = workspace_root_override {
        settings.workspace_root = root.trim().to_string();
    }
    let prior_provider = settings.active_provider_id.clone();
    let prior_model = settings.model.clone();
    let mode = resolve_sub_agent_performance_mode(&settings, sub_agent_id, performance_mode);
    let applied = apply_agent_model_defaults_for_mode(&mut settings, sub_agent_id, mode);
    if !applied {
        log::info!(
            "sub_agent model resolved agent_id={} mode={} provider={} model={} (no override; parent kept)",
            sub_agent_id.trim(),
            mode,
            settings.active_provider_id,
            settings.model
        );
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
    log::info!(
        "sub_agent model resolved agent_id={} mode={} provider={} model={} provider_switched={}",
        sub_agent_id.trim(),
        mode,
        settings.active_provider_id,
        settings.model,
        provider_switched
    );
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
        // coder 本地档位 = fast；平台下发 agentModeLlm 后 fast → deepseek-v4-flash。
        settings
            .agent_performance_modes
            .insert("coder".into(), "fast".into());
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
    fn lead_without_local_tier_falls_back_to_standard_tier() {
        let mut settings = sample_settings();
        settings.providers[0].models = vec!["qwen-fast".into(), "qwen-std".into()];
        settings.agent_mode_llm.insert(
            "general".into(),
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
                    "standard".into(),
                    ComputerTierLlmConfig {
                        provider_id: "qwen".into(),
                        model: "qwen-std".into(),
                        enable_thinking: true,
                        thinking_budget: Some(2048),
                        reasoning_effort: None,
                        thinking_intensity: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        );
        // No `agentPerformanceModes["general"]` entry → the agent fallback is
        // `standard`, so the lead lands on the standard tier (was `fast`).
        assert!(!settings.agent_performance_modes.contains_key("general"));
        assert!(apply_agent_model_defaults(&mut settings, "general"));
        assert_eq!(settings.model, "qwen-std");
    }

    #[test]
    fn prepare_session_falls_back_to_active_when_mode_provider_has_no_key() {
        let mut settings = sample_settings();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        // coder fast → deepseek (not in providers / no key); active qwen has key.
        let key = prepare_session_llm_settings(&mut settings, None, None);
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
        let key = prepare_session_llm_settings(&mut settings, None, None);
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
        let key = prepare_session_llm_settings(&mut settings, None, None);
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.model, "qwen3.6-27b");
        assert_eq!(key, "qwen-key");
    }

    #[test]
    fn prepare_session_keeps_mode_model_when_provider_has_key() {
        let mut settings = sample_settings();
        // coder 本地档位 = fast，且 fast 档 provider 有 key，应保留档位模型。
        settings
            .agent_performance_modes
            .insert("coder".into(), "fast".into());
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
        let key = prepare_session_llm_settings(&mut settings, None, None);
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
        let sub = sub_agent_provider(&parent, "explore", None);
        assert_eq!(sub.settings.active_provider_id, "qwen");
        assert_eq!(sub.settings.model, "qwen-plus");
        assert_eq!(sub.api_key, "qwen-key");
    }

    #[test]
    fn sub_agent_provider_uses_per_agent_defaults() {
        let parent = OpenAIProvider::new(sample_settings(), "parent-key".into());
        let sub = sub_agent_provider(&parent, "explore", None);
        assert_eq!(sub.settings.active_provider_id, "openai");
        assert_eq!(sub.settings.model, "gpt-4o");
        assert_eq!(sub.api_key, "openai-key");
    }

    #[test]
    fn sub_agent_provider_keeps_parent_when_no_override() {
        let parent = OpenAIProvider::new(sample_settings(), "parent-key".into());
        let sub = sub_agent_provider(&parent, "computer", None);
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
        let key = prepare_session_llm_settings(&mut settings, None, None);
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
        let key = prepare_session_llm_settings(&mut settings, Some("explore"), None);
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
        let key = prepare_session_llm_settings(&mut settings, None, Some("expert"));
        // The session override drives the lead's model but must NOT be written into
        // the local per-agent map (it would leak onto same-id sub-agents / self forks).
        assert_eq!(
            settings
                .agent_performance_modes
                .get("coder")
                .map(String::as_str),
            Some("fast")
        );
        assert_eq!(settings.model, "qwen-expert");
        assert_eq!(key, "qwen-key");
    }

    #[test]
    fn session_override_does_not_leak_onto_same_id_sub_agent() {
        // coder session runs `standard`, but coder's *local* tier is `fast`:
        // the lead follows the session override, a coder sub-agent follows local.
        let mut settings = sample_settings();
        settings.api_key = "qwen-key".into();
        settings.lead_agent_id = "coder".into();
        settings.providers[0].models = vec!["qwen-fast".into(), "qwen-std".into()];
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
                    "standard".into(),
                    ComputerTierLlmConfig {
                        provider_id: "qwen".into(),
                        model: "qwen-std".into(),
                        enable_thinking: true,
                        thinking_budget: Some(2048),
                        reasoning_effort: None,
                        thinking_intensity: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        );
        prepare_session_llm_settings(&mut settings, None, Some("standard"));
        assert_eq!(settings.model, "qwen-std");
        let parent = OpenAIProvider::new(settings, "qwen-key".into());
        // Same-id sub-agent resolves the local `fast` tier, not the session `standard`.
        let sub = sub_agent_provider(&parent, "coder", None);
        assert_eq!(sub.settings.model, "qwen-fast");
    }

    #[test]
    fn sub_agent_explicit_mode_beats_local_setting() {
        let mut settings = sample_settings();
        settings
            .agent_performance_modes
            .insert("explore".into(), "fast".into());
        settings.providers[0].models = vec!["qwen-fast".into(), "qwen-expert".into()];
        settings.agent_mode_llm.insert(
            "explore".into(),
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
        let parent = OpenAIProvider::new(settings, "qwen-key".into());
        let sub = sub_agent_provider(&parent, "explore", Some("expert"));
        assert_eq!(sub.settings.model, "qwen-expert");
        // Alias resolves the same tier.
        let aliased = sub_agent_provider(&parent, "explore", Some("advanced"));
        assert_eq!(aliased.settings.model, "qwen-expert");
    }

    #[test]
    fn sub_agent_without_local_tier_falls_back_to_standard() {
        let mut settings = sample_settings();
        settings.providers[0].models = vec!["qwen-fast".into(), "qwen-std".into()];
        settings.agent_mode_llm.insert(
            "explore".into(),
            [(
                "standard".into(),
                ComputerTierLlmConfig {
                    provider_id: "qwen".into(),
                    model: "qwen-std".into(),
                    enable_thinking: true,
                    thinking_budget: Some(2048),
                    reasoning_effort: None,
                    thinking_intensity: None,
                },
            )]
            .into_iter()
            .collect(),
        );
        // `explore` has no local `agentPerformanceModes` entry: built-in `standard`,
        // not the global `fast` default.
        let parent = OpenAIProvider::new(settings, "qwen-key".into());
        let sub = sub_agent_provider(&parent, "explore", None);
        assert_eq!(sub.settings.model, "qwen-std");
    }
}
