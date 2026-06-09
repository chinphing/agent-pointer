//! Record media generation usage for platform token reporting.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::LlmUsageSnapshot;
use crate::media_generation::models::GenerationKind;
use crate::token_usage_store;
use uuid::{uuid, Uuid};

const GENERATION_INSTANCE_NAMESPACE: Uuid = uuid!("b3e7d4a1-9c2f-4e8b-a1d5-6f0c2e8a4b7d");

#[derive(Debug, Clone)]
pub struct GenerationUsage {
    /// Provider-reported total tokens when available (DashScope image/video, Seedance 2.x).
    pub total_tokens: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Images or video seconds for per-unit billing fallback.
    pub unit_count: u32,
    pub billing_mode: GenerationBillingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationBillingMode {
    /// DashScope returns `usage.total_tokens` (new unified token billing).
    ProviderTokens,
    /// Per generated image (Seedream etc.).
    PerImage,
    /// Per video second (Seedance 1.x list price).
    PerVideoSecond,
    /// Seedance 2.0 style: million-token video generation (reserved; API gated).
    PerVideoGenerationToken,
}

impl GenerationUsage {
    pub fn from_dashscope_usage(usage: &serde_json::Value, kind: GenerationKind) -> Self {
        let total = usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let prompt = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let completion = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let unit_count = match kind {
            GenerationKind::Image => usage
                .get("image_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32,
            GenerationKind::Video => usage
                .get("video_duration")
                .or_else(|| usage.get("duration"))
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as u32,
        };
        Self {
            total_tokens: total.max(unit_count),
            prompt_tokens: prompt,
            completion_tokens: completion,
            unit_count,
            billing_mode: if total > 0 {
                GenerationBillingMode::ProviderTokens
            } else {
                match kind {
                    GenerationKind::Image => GenerationBillingMode::PerImage,
                    GenerationKind::Video => GenerationBillingMode::PerVideoSecond,
                }
            },
        }
    }

    pub fn per_image(count: u32) -> Self {
        Self {
            total_tokens: count.saturating_mul(10_000),
            prompt_tokens: 0,
            completion_tokens: count.saturating_mul(10_000),
            unit_count: count,
            billing_mode: GenerationBillingMode::PerImage,
        }
    }

    pub fn per_video_seconds(seconds: u32) -> Self {
        // Seedance 1.x ~0.015元/千tokens ≈ 1元/秒 rough mapping for 2M token budget per 15s clip.
        let pseudo = seconds.saturating_mul(20_000);
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
    let model_key = format!(
        "{}@{}",
        model.trim(),
        match usage.billing_mode {
            GenerationBillingMode::ProviderTokens => "tokens",
            GenerationBillingMode::PerImage => "per-image",
            GenerationBillingMode::PerVideoSecond => "per-sec",
            GenerationBillingMode::PerVideoGenerationToken => "video-tokens",
        }
    );
    if let Err(e) = token_usage_store::record_round(&scope, Some(&snapshot), Some(&model_key)) {
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
