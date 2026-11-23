mod billing;
mod dashscope;
mod models;
mod reference_image;
mod save;
mod volcengine;

pub use reference_image::{
    resolve_reference_image_for_api, resolve_reference_image_for_api_in_env,
};

pub use billing::{GenerationBillingMode, GenerationUsage};
pub use dashscope::{GenerationArtifact, ImageGenerateRequest, VideoGenerateRequest};
pub use models::{
    dashscope_aigc_origin, dashscope_multimodal_image_url, find_volcengine_provider,
    resolve_generation_config, GenerationKind, ResolvedGenerationConfig,
    DOUBAO_DEFAULT_IMAGE_MODEL, DOUBAO_DEFAULT_VIDEO_MODEL, QWEN_DEFAULT_IMAGE_MODEL,
    QWEN_DEFAULT_VIDEO_MODEL,
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

pub fn format_generation_tool_result(
    artifact: &GenerationArtifact,
    kind: GenerationKind,
    conversation_id: &str,
) -> String {
    let label = match kind {
        GenerationKind::Image => "image",
        GenerationKind::Video => "video",
    };
    let count = if artifact.storage_rels.is_empty() {
        artifact.local_paths.len()
    } else {
        artifact.storage_rels.len()
    };
    let mut lines = vec![format!(
        "Generated {count} {label}(s) with {}/{}.",
        artifact.provider, artifact.model
    )];
    let sources: Vec<&str> = if artifact.storage_rels.is_empty() {
        artifact.local_paths.iter().map(String::as_str).collect()
    } else {
        artifact.storage_rels.iter().map(String::as_str).collect()
    };
    for raw in sources {
        match crate::media::store::model_visible_path(conversation_id, raw) {
            Ok(path) => lines.push(format!("MEDIA:{path}")),
            Err(error) => {
                log::warn!(
                    "generation tool result: session path unavailable conversation={conversation_id}: {error:#}"
                );
            }
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(local: &str, rel: Option<&str>) -> GenerationArtifact {
        GenerationArtifact {
            local_paths: vec![local.to_string()],
            storage_rels: rel.into_iter().map(str::to_string).collect(),
            model: "m".into(),
            provider: "p".into(),
            usage: GenerationUsage::per_image(1),
        }
    }

    #[test]
    fn generation_tool_result_uses_container_data_root() {
        let id = format!("gen-path-{}", uuid::Uuid::new_v4());
        let inner = std::sync::Arc::new(crate::env::docker::DockerEnv::fake_for_tests("gen-path"));
        crate::env::session_env::remember_env_for_test(&id, inner);
        let text = format_generation_tool_result(
            &artifact(
                "/private/tmp/pointer-chat-verify/docker-data/users/u/data/a.png",
                Some("data:attachments/conv/a.png"),
            ),
            GenerationKind::Image,
            &id,
        );
        assert!(
            text.contains("MEDIA:/data/attachments/conv/a.png"),
            "{text}"
        );
        assert!(!text.contains("pointer-chat-verify"), "{text}");
    }

    #[test]
    fn generation_tool_result_omits_unmapped_host_path() {
        let text = format_generation_tool_result(
            &artifact("/etc/passwd", None),
            GenerationKind::Image,
            "no-such-generation-conversation",
        );
        assert!(!text.contains("/etc/passwd"), "{text}");
        assert!(!text.contains("MEDIA:"), "{text}");
    }
}
