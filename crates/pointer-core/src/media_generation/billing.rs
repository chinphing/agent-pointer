//! Record media generation usage for platform token reporting.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::media_generation::models::GenerationKind;
use crate::token_usage_store;
use uuid::{uuid, Uuid};

const GENERATION_INSTANCE_NAMESPACE: Uuid = uuid!("b3e7d4a1-9c2f-4e8b-a1d5-6f0c2e8a4b7d");

/// Pseudo tokens per billable image for platform upload (`unit_count` carries the real count).
pub const PSEUDO_TOKENS_PER_IMAGE: u32 = 10_000;
/// Pseudo tokens per billable video second for platform upload.
pub const PSEUDO_TOKENS_PER_VIDEO_SECOND: u32 = 20_000;

#[derive(Debug, Clone)]
pub struct GenerationUsage {
    /// Pseudo tokens for platform store; see `unit_count` for billable images/seconds.
    pub total_tokens: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Images or video seconds for per-unit billing fallback.
    pub unit_count: u32,
    pub billing_mode: GenerationBillingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationBillingMode {
    /// LLM and other token-metered APIs (`model@tokens` or no suffix).
    ProviderTokens,
    /// Per successfully generated image (`model@per-image`; official bills by sheet count).
    PerImage,
    /// Per output video second (`model@per-sec`; HappyHorse, Wan, Seedance).
    PerVideoSecond,
}

impl GenerationBillingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderTokens => "tokens",
            Self::PerImage => "per-image",
            Self::PerVideoSecond => "per-sec",
        }
    }
}

impl GenerationUsage {
    pub fn from_dashscope_usage(usage: &serde_json::Value, kind: GenerationKind) -> Self {
        match kind {
            GenerationKind::Video => Self::from_dashscope_video_usage(usage, 5),
            GenerationKind::Image => Self::from_dashscope_image_usage(usage),
        }
    }

    /// DashScope image (Wan / Qwen Image): billed per `image_count`; API tokens are not billed.
    pub fn from_dashscope_image_usage(usage: &serde_json::Value) -> Self {
        let unit_count = usage
            .get("image_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as u32;
        Self::per_image(unit_count)
    }

    /// DashScope video (HappyHorse / Wan): `usage.duration` is billed per second (official docs).
    pub fn from_dashscope_video_usage(usage: &serde_json::Value, fallback_seconds: u32) -> Self {
        let seconds = usage
            .get("duration")
            .or_else(|| usage.get("output_video_duration"))
            .or_else(|| usage.get("video_duration"))
            .and_then(|v| v.as_u64())
            .unwrap_or(fallback_seconds as u64) as u32;
        Self::per_video_seconds(seconds.max(1))
    }

    pub fn per_image(count: u32) -> Self {
        let count = count.max(1);
        let pseudo = count.saturating_mul(PSEUDO_TOKENS_PER_IMAGE);
        Self {
            total_tokens: pseudo,
            prompt_tokens: 0,
            completion_tokens: pseudo,
            unit_count: count,
            billing_mode: GenerationBillingMode::PerImage,
        }
    }

    pub fn per_video_seconds(seconds: u32) -> Self {
        let seconds = seconds.max(1);
        let pseudo = seconds.saturating_mul(PSEUDO_TOKENS_PER_VIDEO_SECOND);
        Self {
            total_tokens: pseudo,
            prompt_tokens: 0,
            completion_tokens: pseudo,
            unit_count: seconds,
            billing_mode: GenerationBillingMode::PerVideoSecond,
        }
    }

    pub fn to_llm_snapshot(&self) -> LlmUsageSnapshot {
        LlmUsageSnapshot {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens.max(self.total_tokens),
            total_tokens: self.total_tokens,
            reasoning_tokens: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dashscope_video_usage_uses_duration_seconds() {
        let usage = GenerationUsage::from_dashscope_video_usage(
            &json!({
                "duration": 8,
                "output_video_duration": 8,
                "SR": 720
            }),
            5,
        );
        assert_eq!(usage.unit_count, 8);
        assert_eq!(usage.billing_mode, GenerationBillingMode::PerVideoSecond);
    }

    #[test]
    fn dashscope_image_usage_ignores_api_tokens_and_uses_image_count() {
        let usage = GenerationUsage::from_dashscope_image_usage(&json!({
            "image_count": 2,
            "input_tokens": 10867,
            "output_tokens": 2,
            "total_tokens": 10869
        }));
        assert_eq!(usage.unit_count, 2);
        assert_eq!(usage.billing_mode, GenerationBillingMode::PerImage);
        assert_eq!(usage.total_tokens, 2 * PSEUDO_TOKENS_PER_IMAGE);
    }
}

pub fn generation_instance_id(kind: GenerationKind, run_id: &str) -> String {
    let role = match kind {
        GenerationKind::Image => "media-image-generate",
        GenerationKind::Video => "media-video-generate",
    };
    let seed = format!("{role}:{run_id}");
    Uuid::new_v5(&GENERATION_INSTANCE_NAMESPACE, seed.as_bytes()).to_string()
}

pub fn generation_role_id(kind: GenerationKind) -> &'static str {
    match kind {
        GenerationKind::Image => "media-image-generate",
        GenerationKind::Video => "media-video-generate",
    }
}

pub fn record_generation_usage(
    run_id: &str,
    conversation_id: &str,
    kind: GenerationKind,
    model: &str,
    usage: &GenerationUsage,
) {
    let instance_id = generation_instance_id(kind, run_id);
    let scope = AgentInstanceScope::with_instance_id(
        run_id,
        conversation_id,
        generation_role_id(kind),
        &instance_id,
    );
    let snapshot = usage.to_llm_snapshot();
    let model_key = format!("{}@{}", model.trim(), usage.billing_mode.as_str());
    let billing = token_usage_store::UsageBillingMeta {
        billing_mode: usage.billing_mode.as_str(),
        unit_count: usage.unit_count,
    };
    if let Err(e) =
        token_usage_store::record_round(&scope, Some(&snapshot), Some(&model_key), Some(&billing))
    {
        log::warn!(
            "token_usage_store: media generation record failed {} kind={}: {e}",
            scope.log_suffix(),
            generation_role_id(kind)
        );
    } else {
        log::info!(
            "media generation usage {} kind={} model={} mode={:?} units={} total_tokens={}",
            scope.log_suffix(),
            generation_role_id(kind),
            model,
            usage.billing_mode,
            usage.unit_count,
            usage.total_tokens
        );
    }
}
