//! Performance mode → LLM resolution for general/coder agents and media understanding.

use crate::models::{AgentModelRef, ComputerTierLlmConfig, ModelSettings};

pub const PERFORMANCE_MODE_FAST: &str = "fast";
pub const PERFORMANCE_MODE_STANDARD: &str = "standard";
pub const PERFORMANCE_MODE_EXPERT: &str = "expert";

/// Strict parse of a performance mode (aliases included).
///
/// Returns `None` for anything unknown so a caller can **reject** an explicit
/// request instead of silently downgrading it. [`normalize_performance_mode`]
/// keeps the lenient behaviour for stored settings.
pub fn parse_performance_mode(input: &str) -> Option<&'static str> {
    match input.trim().to_ascii_lowercase().as_str() {
        "fast" | "quick" | "primary" => Some(PERFORMANCE_MODE_FAST),
        "standard" => Some(PERFORMANCE_MODE_STANDARD),
        "expert" | "advanced" | "high" => Some(PERFORMANCE_MODE_EXPERT),
        _ => None,
    }
}

/// Lenient normalisation for stored settings: unknown → `standard`.
pub fn normalize_performance_mode(mode: &str) -> &'static str {
    parse_performance_mode(mode).unwrap_or(PERFORMANCE_MODE_STANDARD)
}

/// Sub-agent mode chain: explicit `performanceMode` > local
/// `agentPerformanceModes[agent_id]` > built-in `standard`.
///
/// Kept separate from [`agent_performance_mode`] so the caller-specific fallback
/// (agent `standard` vs. non-agent scene `fast`) can never drift. Returns the
/// effective tier only — the tier's origin is an internal detail and is not
/// surfaced to callers.
pub fn resolve_sub_agent_performance_mode(
    settings: &ModelSettings,
    agent_id: &str,
    explicit: Option<&str>,
) -> &'static str {
    if let Some(mode) = explicit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(parse_performance_mode)
    {
        return mode;
    }
    let key = agent_id.trim();
    if !key.is_empty() {
        if let Some(raw) = settings.agent_performance_modes.get(key) {
            if !raw.trim().is_empty() {
                return normalize_performance_mode(raw);
            }
        }
    }
    PERFORMANCE_MODE_STANDARD
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

/// Stored tier for `agent_id`, falling back to `default_mode` when unset.
///
/// The fallback is a **caller** decision, not a global constant: agents default to
/// [`PERFORMANCE_MODE_STANDARD`] while non-agent scenes (e.g. the `web_search`
/// tool) keep [`PERFORMANCE_MODE_FAST`]. Prefer
/// [`agent_performance_mode`] for agents and this form for scenes.
pub fn agent_performance_mode_with_default(
    settings: &ModelSettings,
    agent_id: &str,
    default_mode: &'static str,
) -> &'static str {
    settings
        .agent_performance_modes
        .get(agent_id.trim())
        .map(|s| s.as_str())
        .map(normalize_performance_mode)
        .unwrap_or(default_mode)
}

/// Agent tier for `agent_id`; unset → [`PERFORMANCE_MODE_STANDARD`].
pub fn agent_performance_mode(settings: &ModelSettings, agent_id: &str) -> &'static str {
    agent_performance_mode_with_default(settings, agent_id, PERFORMANCE_MODE_STANDARD)
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
    if let Some(cfg) = resolve_agent_mode_llm_config(settings, agent_id) {
        return Some(tier_cfg_to_ref(cfg));
    }
    settings.agent_default_models.get(agent_id.trim()).cloned()
}

/// Resolve the `agentModeLlm` entry for an explicit `mode`.
///
/// Support is decided by the data — the agent has an entry for that mode — not
/// by a hard-coded agent allow-list. A worker such as `explore` therefore
/// resolves its tier as soon as one is configured, and an agent with no entry
/// simply falls through to `agentDefaultModels`.
pub fn resolve_agent_mode_llm_config_for_mode<'a>(
    settings: &'a ModelSettings,
    agent_id: &str,
    mode: &str,
) -> Option<&'a ComputerTierLlmConfig> {
    let key = agent_id.trim();
    if key.is_empty() {
        return None;
    }
    let inner = settings.agent_mode_llm.get(key)?;
    let cfg = inner.get(mode)?;
    if cfg.model.trim().is_empty() {
        return None;
    }
    Some(cfg)
}

pub fn resolve_agent_mode_llm_config<'a>(
    settings: &'a ModelSettings,
    agent_id: &str,
) -> Option<&'a ComputerTierLlmConfig> {
    let key = agent_id.trim();
    if key.is_empty() {
        return None;
    }
    let mode = agent_performance_mode(settings, key);
    resolve_agent_mode_llm_config_for_mode(settings, key, mode)
}

pub fn resolve_media_mode_llm_config<'a>(
    settings: &'a ModelSettings,
    kind: &str,
) -> Option<&'a ComputerTierLlmConfig> {
    let kind = kind.trim();
    let mode = media_understanding_mode(settings, kind);
    let inner = settings.media_mode_llm.get(kind)?;
    let cfg = inner.get(mode)?;
    if cfg.model.trim().is_empty() {
        return None;
    }
    Some(cfg)
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
                reasoning_effort: None,
                thinking_intensity: None,
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

    #[test]
    fn parse_performance_mode_accepts_aliases_and_rejects_unknown() {
        assert_eq!(parse_performance_mode("fast"), Some(PERFORMANCE_MODE_FAST));
        assert_eq!(
            parse_performance_mode("  QUICK "),
            Some(PERFORMANCE_MODE_FAST)
        );
        assert_eq!(
            parse_performance_mode("primary"),
            Some(PERFORMANCE_MODE_FAST)
        );
        assert_eq!(
            parse_performance_mode("standard"),
            Some(PERFORMANCE_MODE_STANDARD)
        );
        assert_eq!(
            parse_performance_mode("expert"),
            Some(PERFORMANCE_MODE_EXPERT)
        );
        assert_eq!(
            parse_performance_mode("advanced"),
            Some(PERFORMANCE_MODE_EXPERT)
        );
        assert_eq!(parse_performance_mode("high"), Some(PERFORMANCE_MODE_EXPERT));
        // Unknown input must not silently normalise when parsed strictly.
        assert_eq!(parse_performance_mode("turbo"), None);
        assert_eq!(parse_performance_mode(""), None);
        // Lenient normalisation keeps the legacy `standard` fallback.
        assert_eq!(normalize_performance_mode("turbo"), PERFORMANCE_MODE_STANDARD);
    }

    #[test]
    fn sub_agent_mode_prefers_explicit_then_local_then_standard() {
        let mut settings = ModelSettings::default();
        // No local setting, no explicit → built-in standard (not the global fast).
        assert_eq!(
            resolve_sub_agent_performance_mode(&settings, "coder", None),
            PERFORMANCE_MODE_STANDARD
        );
        settings
            .agent_performance_modes
            .insert("coder".into(), "fast".into());
        assert_eq!(
            resolve_sub_agent_performance_mode(&settings, "coder", None),
            PERFORMANCE_MODE_FAST
        );
        // Explicit wins over the local setting; aliases normalise.
        assert_eq!(
            resolve_sub_agent_performance_mode(&settings, "coder", Some("high")),
            PERFORMANCE_MODE_EXPERT
        );
        // A different agent keeps the built-in standard.
        assert_eq!(
            resolve_sub_agent_performance_mode(&settings, "explore", None),
            PERFORMANCE_MODE_STANDARD
        );
    }

    #[test]
    fn explore_resolves_mode_llm_without_agent_allow_list() {
        let mut settings = ModelSettings::default();
        settings.agent_mode_llm = sample_mode_llm("explore", "standard", "explore-std");
        // No hard-coded agent id: the entry itself makes the tier resolvable.
        let r = resolve_agent_mode_llm_config_for_mode(&settings, "explore", "standard").unwrap();
        assert_eq!(r.model, "explore-std");
        // An agent with no entry stays unresolved.
        assert!(resolve_agent_mode_llm_config_for_mode(&settings, "computer", "standard").is_none());
        assert!(resolve_agent_mode_llm_config_for_mode(&settings, "explore", "expert").is_none());
    }

    #[test]
    fn agent_performance_mode_defaults_to_standard_when_unset() {
        let settings = ModelSettings::default();
        // Agent-side fallback is `standard` (was `fast`).
        assert_eq!(
            agent_performance_mode(&settings, "general"),
            PERFORMANCE_MODE_STANDARD
        );
        assert_eq!(
            agent_performance_mode(&settings, "explore"),
            PERFORMANCE_MODE_STANDARD
        );
        // An explicit local value still wins.
        let mut with_local = ModelSettings::default();
        with_local
            .agent_performance_modes
            .insert("coder".into(), "fast".into());
        assert_eq!(agent_performance_mode(&with_local, "coder"), PERFORMANCE_MODE_FAST);
    }

    #[test]
    fn non_agent_scene_keeps_fast_fallback() {
        let settings = ModelSettings::default();
        // Caller-supplied default: the `web_search` tool scene stays `fast`.
        assert_eq!(
            agent_performance_mode_with_default(
                &settings,
                "web_search",
                PERFORMANCE_MODE_FAST
            ),
            PERFORMANCE_MODE_FAST
        );
        // …and an explicit scene tier still wins.
        let mut with_local = ModelSettings::default();
        with_local
            .agent_performance_modes
            .insert("web_search".into(), "expert".into());
        assert_eq!(
            agent_performance_mode_with_default(
                &with_local,
                "web_search",
                PERFORMANCE_MODE_FAST
            ),
            PERFORMANCE_MODE_EXPERT
        );
    }

    #[test]
    fn media_understanding_defaults_to_fast_when_unset() {
        let settings = ModelSettings::default();
        assert_eq!(
            media_understanding_mode(&settings, "image"),
            PERFORMANCE_MODE_FAST
        );
        assert_eq!(
            media_understanding_mode(&settings, "audio"),
            PERFORMANCE_MODE_FAST
        );
        assert_eq!(
            media_understanding_mode(&settings, "video"),
            PERFORMANCE_MODE_FAST
        );
    }
}
