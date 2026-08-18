//! Model catalog and provider resolution for image/video generation.

use crate::models::{AgentModelRef, MediaModelOverrides, ModelSettings, ProviderConfig};

// 平台模型配置全部由平台下发；本地不内置媒体生成默认模型
// （由 tierDefaults.mediaGeneration 注入 media_model_overrides；未配置时为空）。
pub const QWEN_DEFAULT_IMAGE_MODEL: &str = "";
pub const QWEN_DEFAULT_VIDEO_MODEL: &str = "";
pub const DOUBAO_DEFAULT_IMAGE_MODEL: &str = "";
pub const DOUBAO_DEFAULT_VIDEO_MODEL: &str = "";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationKind {
    Image,
    Video,
}

#[derive(Debug, Clone)]
pub struct ResolvedGenerationConfig {
    pub provider_id: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    /// Provider source: `platform` | `user` (same as settings provider.source).
    pub source: &'static str,
}

pub fn find_dashscope_provider(settings: &ModelSettings) -> Option<&ProviderConfig> {
    crate::models::find_dashscope_provider(settings)
}

pub fn find_volcengine_provider(settings: &ModelSettings) -> Option<&ProviderConfig> {
    settings.providers.iter().find(|p| {
        let url = p.base_url.to_ascii_lowercase();
        url.contains("volces.com") || url.contains("volcengineapi.com")
    })
}

fn pick_override<'a>(
    overrides: &'a MediaModelOverrides,
    kind: GenerationKind,
) -> Option<&'a AgentModelRef> {
    match kind {
        GenerationKind::Image => overrides
            .image_generation
            .as_ref()
            .or(overrides.image.as_ref()),
        GenerationKind::Video => overrides
            .video_generation
            .as_ref()
            .or(overrides.video.as_ref()),
    }
}

fn default_model_for_provider(provider_id: &str, kind: GenerationKind) -> &'static str {
    let id = provider_id.to_ascii_lowercase();
    match (id.as_str(), kind) {
        ("doubao" | "volcengine" | "ark", GenerationKind::Image) => DOUBAO_DEFAULT_IMAGE_MODEL,
        ("doubao" | "volcengine" | "ark", GenerationKind::Video) => DOUBAO_DEFAULT_VIDEO_MODEL,
        (_, GenerationKind::Image) => QWEN_DEFAULT_IMAGE_MODEL,
        (_, GenerationKind::Video) => QWEN_DEFAULT_VIDEO_MODEL,
    }
}

/// Infer the intended **API dialect** from a model name when providerId is missing.
fn protocol_for_model(model: &str) -> Option<&'static str> {
    let m = model.trim().to_ascii_lowercase();
    if m.starts_with("wan") || m.starts_with("qwen") || m.contains("happyhorse") {
        Some("dashscope")
    } else if m.starts_with("doubao") || m.contains("seedance") || m.contains("seedream") {
        Some("volcengine")
    } else {
        None
    }
}

fn first_generation_provider_id(settings: &ModelSettings) -> String {
    if let Some(p) = find_volcengine_provider(settings) {
        return p.id.clone();
    }
    if let Some(p) = find_dashscope_provider(settings) {
        return p.id.clone();
    }
    settings
        .providers
        .first()
        .map(|p| p.id.clone())
        .unwrap_or_default()
}

/// Provider from settings `mediaModelOverrides.*Generation` only (not tool args).
fn resolve_provider_id(settings: &ModelSettings, model_ref: Option<&AgentModelRef>) -> String {
    if let Some(r) = model_ref {
        let pid = r.provider_id.trim();
        if !pid.is_empty() {
            return pid.to_string();
        }
        let model = r.model.trim();
        if !model.is_empty() {
            match protocol_for_model(model) {
                Some("dashscope") => {
                    if let Some(p) = find_dashscope_provider(settings) {
                        return p.id.clone();
                    }
                }
                Some("volcengine") => {
                    if let Some(p) = find_volcengine_provider(settings) {
                        return p.id.clone();
                    }
                }
                _ => {}
            }
        }
    }
    first_generation_provider_id(settings)
}

pub fn resolve_generation_config(
    settings: &ModelSettings,
    kind: GenerationKind,
) -> anyhow::Result<ResolvedGenerationConfig> {
    let overrides = &settings.media_model_overrides;
    let model_ref = pick_override(overrides, kind);
    let provider_id = resolve_provider_id(settings, model_ref);

    let provider = settings
        .providers
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(&provider_id))
        .or_else(|| {
            if provider_is_volcengine(&provider_id, "") {
                find_volcengine_provider(settings)
            } else {
                find_dashscope_provider(settings)
            }
        })
        .or_else(|| find_dashscope_provider(settings))
        .or_else(|| find_volcengine_provider(settings))
    .ok_or_else(|| {
        anyhow::anyhow!(
            "No {} provider configured. Add a DashScope- or Volcengine-compatible provider in settings.",
            match kind {
                GenerationKind::Image => "image generation",
                GenerationKind::Video => "video generation",
            }
        )
    })?;

    let api_key = provider.api_key.trim();
    if api_key.is_empty() {
        anyhow::bail!(
            "Provider \"{}\" API key is missing. Configure it in settings before generating media.",
            provider.id
        );
    }

    let model = model_ref
        .map(|r| r.model.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_model_for_provider(&provider.id, kind).to_string());

    Ok(ResolvedGenerationConfig {
        provider_id: provider.id.clone(),
        api_key: api_key.to_string(),
        base_url: provider.base_url.trim().trim_end_matches('/').to_string(),
        model,
        source: if provider.source.as_deref()
            == Some(crate::llm_token_stats::PROVIDER_SOURCE_PLATFORM)
        {
            crate::llm_token_stats::PROVIDER_SOURCE_PLATFORM
        } else {
            crate::llm_token_stats::PROVIDER_SOURCE_USER
        },
    })
}

/// DashScope AIGC origin from compatible-mode base URL.
pub fn dashscope_aigc_origin(compatible_base: &str) -> String {
    let trimmed = compatible_base.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    for host in [
        "dashscope.aliyuncs.com",
        "dashscope-intl.aliyuncs.com",
        "dashscope-us.aliyuncs.com",
    ] {
        if lower.contains(host) {
            return format!("https://{host}");
        }
    }
    if lower.ends_with("/compatible-mode/v1") {
        return trimmed.trim_end_matches("/compatible-mode/v1").to_string();
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return trimmed.to_string();
    }
    "https://dashscope.aliyuncs.com".to_string()
}

pub fn dashscope_multimodal_image_url(base_url: &str) -> String {
    format!(
        "{}/api/v1/services/aigc/multimodal-generation/generation",
        dashscope_aigc_origin(base_url)
    )
}

pub fn dashscope_video_synthesis_url(base_url: &str) -> String {
    format!(
        "{}/api/v1/services/aigc/video-generation/video-synthesis",
        dashscope_aigc_origin(base_url)
    )
}

pub fn dashscope_tasks_url(base_url: &str, task_id: &str) -> String {
    format!(
        "{}/api/v1/tasks/{}",
        dashscope_aigc_origin(base_url),
        task_id.trim()
    )
}

pub fn volcengine_ark_origin(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("volces.com") {
        if let Some(idx) = lower.find("/api/") {
            return trimmed[..idx].to_string();
        }
        return trimmed.to_string();
    }
    "https://ark.cn-beijing.volces.com".to_string()
}

pub fn volcengine_image_url(base_url: &str) -> String {
    format!(
        "{}/api/v3/images/generations",
        volcengine_ark_origin(base_url)
    )
}

pub fn volcengine_video_tasks_url(base_url: &str) -> String {
    format!(
        "{}/api/v3/contents/generations/tasks",
        volcengine_ark_origin(base_url)
    )
}

pub fn volcengine_video_task_url(base_url: &str, task_id: &str) -> String {
    format!(
        "{}/api/v3/contents/generations/tasks/{}",
        volcengine_ark_origin(base_url),
        task_id.trim()
    )
}

pub fn provider_is_volcengine(provider_id: &str, base_url: &str) -> bool {
    let url = base_url.to_ascii_lowercase();
    if url.contains("volces.com") || url.contains("volcengineapi.com") {
        return true;
    }
    let id = provider_id.to_ascii_lowercase();
    id == "doubao" || id == "volcengine" || id == "ark"
}

pub fn is_happyhorse_model(model: &str) -> bool {
    model.to_ascii_lowercase().contains("happyhorse")
}

pub fn is_seedance_v2_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("seedance-2-0") || m.contains("seedance-2.0")
}

/// Pick HappyHorse i2v when a first-frame image is supplied but config still points at t2v.
pub fn resolve_dashscope_video_model(model: &str, has_first_frame: bool) -> String {
    let m = model.trim();
    if !is_happyhorse_model(m) {
        return m.to_string();
    }
    if has_first_frame && m.contains("-t2v") {
        return m.replace("-t2v", "-i2v");
    }
    m.to_string()
}

pub fn resolve_volcengine_video_model(model: &str, has_first_frame: bool) -> String {
    let m = model.trim();
    if is_seedance_v2_model(m) {
        return m.to_string();
    }
    if has_first_frame && m.contains("-t2v-") {
        return m.replace("-t2v-", "-i2v-");
    }
    m.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashscope_origin_from_compatible_mode() {
        let origin = dashscope_aigc_origin("https://dashscope.aliyuncs.com/compatible-mode/v1");
        assert_eq!(origin, "https://dashscope.aliyuncs.com");
    }

    #[test]
    fn volcengine_urls() {
        let base = "https://ark.cn-beijing.volces.com/api/v3";
        assert!(volcengine_image_url(base).ends_with("/images/generations"));
        assert!(volcengine_video_tasks_url(base).ends_with("/contents/generations/tasks"));
    }

    #[test]
    fn video_generation_uses_settings_not_tool_model() {
        use crate::models::{AgentModelRef, ProviderConfig};

        let mut settings = ModelSettings::default();
        settings.providers = vec![
            ProviderConfig {
                id: "qwen".into(),
                name: "Qwen".into(),
                api_key: "ds-key".into(),
                base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
                models: vec![],
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
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
                id: "doubao".into(),
                name: "Doubao".into(),
                api_key: "ark-key".into(),
                base_url: "https://ark.cn-beijing.volces.com/api/v3".into(),
                models: vec![],
                reasoning_in_messages: None,
                temperature: None,
                max_tokens: None,
                model_configs: Default::default(),
                enable_thinking: None,
                thinking_budget: None,
                reasoning_effort: None,
                thinking_protocol: None,
                thinking_intensity: None,
                extra_body: None,
                source: None,
            },
        ];
        settings.media_model_overrides.video_generation = Some(AgentModelRef {
            provider_id: "doubao".into(),
            model: "doubao-seedance-2-0-fast-260128".into(),
        });
        let cfg = resolve_generation_config(&settings, GenerationKind::Video).expect("config");
        assert_eq!(cfg.model, "doubao-seedance-2-0-fast-260128");
        assert_eq!(cfg.provider_id, "doubao");
        assert!(cfg.base_url.contains("volces"));
    }

    #[test]
    fn qwen_happyhorse_settings_route_to_dashscope() {
        use crate::models::{AgentModelRef, ProviderConfig};

        let mut settings = ModelSettings::default();
        settings.providers = vec![ProviderConfig {
            id: "qwen".into(),
            name: "Qwen".into(),
            api_key: "ds-key".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            models: vec![],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: Default::default(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            thinking_protocol: None,
            thinking_intensity: None,
            extra_body: None,
            source: None,
        }];
        settings.media_model_overrides.video_generation = Some(AgentModelRef {
            provider_id: "qwen".into(),
            model: "happyhorse-1.0-t2v".into(),
        });
        let cfg = resolve_generation_config(&settings, GenerationKind::Video).expect("config");
        assert_eq!(cfg.model, "happyhorse-1.0-t2v");
        assert_eq!(cfg.provider_id, "qwen");
    }

    #[test]
    fn happyhorse_and_seedance_v2_detection() {
        assert!(is_happyhorse_model("happyhorse-1.0-t2v"));
        assert!(is_seedance_v2_model("doubao-seedance-2-0-260128"));
        assert_eq!(
            resolve_dashscope_video_model("happyhorse-1.0-t2v", true),
            "happyhorse-1.0-i2v"
        );
        assert_eq!(
            resolve_volcengine_video_model("doubao-seedance-2-0-fast-260128", true),
            "doubao-seedance-2-0-fast-260128"
        );
    }
}
