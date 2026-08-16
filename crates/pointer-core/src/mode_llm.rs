//! Performance mode → LLM resolution for general/coder agents and media understanding.

use crate::models::{AgentModelRef, ComputerTierLlmConfig, ModelSettings};

pub const PERFORMANCE_MODE_FAST: &str = "fast";
pub const PERFORMANCE_MODE_STANDARD: &str = "standard";
pub const PERFORMANCE_MODE_EXPERT: &str = "expert";

const MODE_AGENTS: [&str; 2] = ["general", "coder"];

pub fn is_mode_agent(agent_id: &str) -> bool {
    MODE_AGENTS.contains(&agent_id.trim())
}

pub fn normalize_performance_mode(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "fast" | "quick" | "primary" => PERFORMANCE_MODE_FAST,
        "expert" | "advanced" | "high" => PERFORMANCE_MODE_EXPERT,
        _ => PERFORMANCE_MODE_STANDARD,
    }
}

fn tier_cfg_to_ref(cfg: &ComputerTierLlmConfig) -> AgentModelRef {
    AgentModelRef {
        provider_id: cfg.provider_id.trim().to_string(),
        model: cfg.model.trim().to_string(),
    }
}

fn lookup_mode_llm(
    map: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, ComputerTierLlmConfig>,
    >,
    outer_key: &str,
    mode: &str,
) -> Option<AgentModelRef> {
    let inner = map.get(outer_key)?;
    let cfg = inner.get(mode)?;
    if cfg.model.trim().is_empty() {
        return None;
    }
    Some(tier_cfg_to_ref(cfg))
}

pub fn agent_performance_mode(settings: &ModelSettings, agent_id: &str) -> &'static str {
    settings
        .agent_performance_modes
        .get(agent_id.trim())
        .map(|s| s.as_str())
        .map(normalize_performance_mode)
        .unwrap_or(PERFORMANCE_MODE_FAST)
}

pub fn media_understanding_mode(settings: &ModelSettings, kind: &str) -> &'static str {
    let mode = match kind.trim() {
        "audio" => settings.media_understanding_modes.audio.as_deref(),
        "video" => settings.media_understanding_modes.video.as_deref(),
        _ => settings.media_understanding_modes.image.as_deref(),
    };
    normalize_performance_mode(mode.unwrap_or(PERFORMANCE_MODE_FAST))
}

pub fn resolve_agent_mode_llm(settings: &ModelSettings, agent_id: &str) -> Option<AgentModelRef> {
    if !is_mode_agent(agent_id) {
        return None;
    }
    let key = agent_id.trim();
    let mode = agent_performance_mode(settings, key);
    if let Some(r) = lookup_mode_llm(&settings.agent_mode_llm, key, mode) {
        return Some(r);
    }
    settings.agent_default_models.get(key).cloned()
}

pub fn resolve_media_mode_llm(settings: &ModelSettings, kind: &str) -> AgentModelRef {
    let kind = kind.trim();
    let mode = media_understanding_mode(settings, kind);
    if let Some(r) = lookup_mode_llm(&settings.media_mode_llm, kind, mode) {
        return r;
    }
    // Legacy direct overrides (image / audio / video keys in media_model_overrides).
    let legacy = match kind {
        "audio" => settings.media_model_overrides.audio.as_ref(),
        "video" => settings.media_model_overrides.video.as_ref(),
        _ => settings.media_model_overrides.image.as_ref(),
    };
    if let Some(m) = legacy.filter(|m| !m.model.trim().is_empty()) {
        return m.clone();
    }
    // 无本地平台默认：媒体理解未配置时返回空引用，由调用方按“未配置 API Key”处理。
    AgentModelRef {
        provider_id: String::new(),
        model: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaModelOverrides;
    use std::collections::HashMap;

    fn sample_mode_llm(
        agent: &str,
        mode: &str,
        model: &str,
    ) -> HashMap<String, HashMap<String, ComputerTierLlmConfig>> {
        let mut inner = HashMap::new();
        inner.insert(
            mode.into(),
            ComputerTierLlmConfig {
                provider_id: "qwen".into(),
                model: model.into(),
                enable_thinking: true,
                thinking_budget: Some(2048),
            },
        );
        let mut outer = HashMap::new();
        outer.insert(agent.into(), inner);
        outer
    }

    #[test]
    fn resolve_general_fast_mode() {
        let mut settings = ModelSettings::default();
        settings
            .agent_performance_modes
            .insert("general".into(), "fast".into());
        settings.agent_mode_llm = sample_mode_llm("general", "fast", "qwen3.5-flash");
        let r = resolve_agent_mode_llm(&settings, "general").unwrap();
        assert_eq!(r.model, "qwen3.5-flash");
    }

    #[test]
    fn resolve_media_fast_fallback_empty_without_platform_defaults() {
        let settings = ModelSettings::default();
        let r = resolve_media_mode_llm(&settings, "image");
        // 本地不再内置平台默认模型；未配置时返回空引用，由调用方按未配置处理。
        assert!(r.model.is_empty());
        assert!(r.provider_id.is_empty());
    }

    #[test]
    fn resolve_media_legacy_override() {
        let mut settings = ModelSettings::default();
        settings.media_mode_llm.clear();
        settings.media_model_overrides = MediaModelOverrides {
            image: Some(AgentModelRef {
                provider_id: "deepseek".into(),
                model: "deepseek-v4-pro".into(),
            }),
            ..Default::default()
        };
        let r = resolve_media_mode_llm(&settings, "image");
        assert_eq!(r.model, "deepseek-v4-pro");
    }
}
