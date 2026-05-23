use crate::agents::{DEFAULT_LEAD_AGENT_ID, SUPERVISOR_AGENT_ID, AGENT_MODE_SUPERVISOR};
use crate::models::ModelSettings;
use crate::provider::OpenAIProvider;

/// Apply per-agent default model from `agent_default_models` when `agent_id` has an entry.
/// Returns true when provider and/or model were overridden.
pub(crate) fn apply_agent_model_defaults(settings: &mut ModelSettings, agent_id: &str) -> bool {
    let key = agent_id.trim();
    if key.is_empty() {
        return false;
    }
    let Some(pref) = settings.agent_default_models.get(key) else {
        return false;
    };
    let mut applied = false;
    if !pref.provider_id.trim().is_empty() {
        settings.active_provider_id = pref.provider_id.trim().to_string();
        applied = true;
    }
    if !pref.model.trim().is_empty() {
        settings.model = pref.model.trim().to_string();
        applied = true;
    }
    applied
}

pub(crate) fn apply_session_agent_model_defaults(
    settings: &mut ModelSettings,
    effective_agent_mode: &str,
) {
    let mode = effective_agent_mode.trim();
    let key = if mode == AGENT_MODE_SUPERVISOR {
        SUPERVISOR_AGENT_ID.to_string()
    } else {
        let id = settings.lead_agent_id.trim();
        if id.is_empty() {
            DEFAULT_LEAD_AGENT_ID.to_string()
        } else {
            id.to_string()
        }
    };
    let _ = apply_agent_model_defaults(settings, &key);
}

/// Resolve API key for `settings.active_provider_id`, falling back to the parent session key.
pub(crate) fn resolve_provider_api_key(settings: &ModelSettings, fallback_api_key: &str) -> String {
    let pid = settings.active_provider_id.trim();
    if let Some(p) = settings.providers.iter().find(|p| p.id == pid) {
        if !p.api_key.trim().is_empty() {
            return p.api_key.clone();
        }
    }
    fallback_api_key.trim().to_string()
}

/// Build a provider for a sub-agent run, honoring per-agent `agentDefaultModels` when set.
pub(crate) fn sub_agent_provider(parent: &OpenAIProvider, sub_agent_id: &str) -> OpenAIProvider {
    let mut settings = parent.settings.clone();
    let applied = apply_agent_model_defaults(&mut settings, sub_agent_id);
    if !applied {
        return OpenAIProvider::new(settings, parent.api_key.clone());
    }
    let provider_switched =
        settings.active_provider_id.trim() != parent.settings.active_provider_id.trim();
    let api_key = if provider_switched {
        resolve_provider_api_key(&settings, &parent.api_key)
    } else {
        parent.api_key.clone()
    };
    log::info!(
        "sub_agent model override agent_id={} provider={} model={} provider_switched={}",
        sub_agent_id.trim(),
        settings.active_provider_id,
        settings.model,
        provider_switched
    );
    OpenAIProvider::new(settings, api_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AgentModelRef, ProviderConfig};

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
                    max_tokens: None,
                    model_configs: Default::default(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                },
                ProviderConfig {
                    id: "openai".into(),
                    name: "OpenAI".into(),
                    base_url: "https://api.openai.com".into(),
                    api_key: "openai-key".into(),
                    models: vec!["gpt-4o".into()],
                    reasoning_in_messages: None,
                    temperature: None,
                    max_tokens: None,
                    model_configs: Default::default(),
                    enable_thinking: None,
                    thinking_budget: None,
                    reasoning_effort: None,
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
        assert!(!apply_agent_model_defaults(&mut settings, "coder"));
        assert_eq!(settings.active_provider_id, "qwen");
        assert_eq!(settings.model, "qwen-plus");
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
        let sub = sub_agent_provider(&parent, "coder");
        assert_eq!(sub.settings.active_provider_id, "qwen");
        assert_eq!(sub.settings.model, "qwen-plus");
        assert_eq!(sub.api_key, "parent-key");
    }
}
