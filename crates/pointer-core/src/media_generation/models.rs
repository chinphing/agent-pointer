//! Model catalog and provider resolution for image/video generation.

use crate::models::{AgentModelRef, MediaModelOverrides, ModelSettings, ProviderConfig};

/// DashScope 万相 2.7 — unified image gen/edit (sync multimodal API).
pub const QWEN_DEFAULT_IMAGE_MODEL: &str = "wan2.7-image-pro";

/// HappyHorse 1.0 文生视频（官网推荐，原生音画同步）。
pub const QWEN_DEFAULT_VIDEO_MODEL: &str = "happyhorse-1.0-t2v";

/// 火山方舟 Seedream 5.0 Lite（默认图片生成）。
pub const DOUBAO_DEFAULT_IMAGE_MODEL: &str = "doubao-seedream-5-0-lite-260128";
/// Seedance 1.0 Lite 文生视频（有首帧时自动切 `…-lite-i2v-…`）。
pub const DOUBAO_DEFAULT_VIDEO_MODEL: &str = "doubao-seedance-1-0-lite-t2v-250428";

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
}

pub fn find_dashscope_provider(settings: &ModelSettings) -> Option<&ProviderConfig> {
    crate::models::find_dashscope_provider(settings)
}

pub fn find_volcengine_provider(settings: &ModelSettings) -> Option<&ProviderConfig> {
    settings
        .providers
        .iter()
        .find(|p| {
            let id = p.id.to_ascii_lowercase();
            id == "doubao" || id == "volcengine" || id == "ark"
        })
        .or_else(|| {
            settings.providers.iter().find(|p| {
                let url = p.base_url.to_ascii_lowercase();
                url.contains("volces.com") || url.contains("volcengineapi.com")
            })
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

/// Infer the intended provider from a model name string.
/// Returns `Some("qwen")` for DashScope models or `Some("doubao")` for Volcengine models.
fn provider_for_model(model: &str) -> Option<&'static str> {
    let m = model.trim().to_ascii_lowercase();
    if m.starts_with("wan") || m.starts_with("qwen") || m.contains("happyhorse") {
        Some("qwen")
    } else if m.starts_with("doubao") {
        Some("doubao")
    } else {
        None
    }
}

fn provider_configured(settings: &ModelSettings, pid: &str) -> bool {
    match pid {
        "qwen" => find_dashscope_provider(settings).is_some(),
        "doubao" => find_volcengine_provider(settings).is_some(),
        _ => false,
    }
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
            if let Some(pid) = provider_for_model(model) {
                if provider_configured(settings, pid) {
                    return pid.to_string();
                }
            }
        }
    }
    if find_volcengine_provider(settings).is_some() {
        "doubao".into()
    } else {
        "qwen".into()
    }
}

pub fn resolve_generation_config(
    settings: &ModelSettings,
    kind: GenerationKind,
) -> anyhow::Result<ResolvedGenerationConfig> {
    let overrides = &settings.media_model_overrides;
    let model_ref = pick_override(overrides, kind);
    let provider_id = resolve_provider_id(settings, model_ref);

    let provider = if provider_id.eq_ignore_ascii_case("doubao")
        || provider_id.eq_ignore_ascii_case("volcengine")
        || provider_id.eq_ignore_ascii_case("ark")
    {
        find_volcengine_provider(settings)
    } else {
        find_dashscope_provider(settings)
    }
    .ok_or_else(|| {
        anyhow::anyhow!(
            "No {} provider configured. Add Qwen (DashScope) or Doubao (Volcengine Ark) in settings.",
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
        return trimmed
            .trim_end_matches("/compatible-mode/v1")
            .to_string();
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
    format!("{}/api/v3/images/generations", volcengine_ark_origin(base_url))
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
    let id = provider_id.to_ascii_lowercase();
    if id == "doubao" || id == "volcengine" || id == "ark" {
        return true;
    }
    base_url.to_ascii_lowercase().contains("volces.com")
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
            },
        ];
        settings.media_model_overrides.video_generation = Some(AgentModelRef {
            provider_id: "doubao".into(),
            model: "doubao-seedance-1-0-lite-t2v-250428".into(),
        });
        let cfg =
            resolve_generation_config(&settings, GenerationKind::Video).expect("config");
        assert_eq!(cfg.model, "doubao-seedance-1-0-lite-t2v-250428");
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
        }];
        settings.media_model_overrides.video_generation = Some(AgentModelRef {
            provider_id: "qwen".into(),
            model: "happyhorse-1.0-t2v".into(),
        });
        let cfg =
            resolve_generation_config(&settings, GenerationKind::Video).expect("config");
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
            resolve_volcengine_video_model("doubao-seedance-1-0-lite-t2v-250428", true),
            "doubao-seedance-1-0-lite-i2v-250428"
        );
    }
}
