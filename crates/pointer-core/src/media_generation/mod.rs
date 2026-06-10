mod billing;
mod dashscope;
mod models;
mod reference_image;
mod save;
mod volcengine;

pub use reference_image::resolve_reference_image_for_api;

pub use billing::{GenerationBillingMode, GenerationUsage};
pub use dashscope::{GenerationArtifact, ImageGenerateRequest, VideoGenerateRequest};
pub use models::{
    dashscope_aigc_origin, find_volcengine_provider, resolve_generation_config,
    GenerationKind, ResolvedGenerationConfig, DOUBAO_DEFAULT_IMAGE_MODEL,
    DOUBAO_DEFAULT_VIDEO_MODEL, QWEN_DEFAULT_IMAGE_MODEL, QWEN_DEFAULT_VIDEO_MODEL,
};

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::models::ModelSettings;

pub async fn generate_image(
    settings: &ModelSettings,
    conversation_id: &str,
    run_id: &str,
    req: &ImageGenerateRequest,
    cancel: CancellationToken,
) -> Result<GenerationArtifact> {
    let cfg = resolve_generation_config(settings, GenerationKind::Image)?;
    if volcengine::route_volcengine(&cfg) {
        volcengine::generate_image_volcengine(&cfg, conversation_id, run_id, req, cancel).await
    } else {
        dashscope::generate_image_dashscope(&cfg, conversation_id, run_id, req, cancel).await
    }
}

pub async fn generate_video(
    settings: &ModelSettings,
    conversation_id: &str,
    run_id: &str,
    req: &VideoGenerateRequest,
    cancel: CancellationToken,
) -> Result<GenerationArtifact> {
    let cfg = resolve_generation_config(settings, GenerationKind::Video)?;
    if volcengine::route_volcengine(&cfg) {
        volcengine::generate_video_volcengine(&cfg, conversation_id, run_id, req, cancel).await
    } else {
        dashscope::generate_video_dashscope(&cfg, conversation_id, run_id, req, cancel).await
    }
}

pub fn format_generation_tool_result(artifact: &GenerationArtifact, kind: GenerationKind) -> String {
    let label = match kind {
        GenerationKind::Image => "image",
        GenerationKind::Video => "video",
    };
    let mut lines = vec![format!(
        "Generated {} {}(s) with {}/{}.",
        artifact.local_paths.len(),
        label,
        artifact.provider,
        artifact.model
    )];
    for path in &artifact.local_paths {
        lines.push(format!("MEDIA:{path}"));
    }
    lines.join("\n")
}
