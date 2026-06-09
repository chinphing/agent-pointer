//! Token usage reporting for multimedia understanding LLM calls.

use crate::agent_instance_scope::AgentInstanceScope;
use crate::llm_token_stats::model_name_for_usage_report;
use crate::provider::ChatOnceOutput;
use crate::token_usage_store;
use uuid::{uuid, Uuid};

/// Stable namespace for per-run media understanding instance ids (must be UUID for platform upload).
const MEDIA_INSTANCE_NAMESPACE: Uuid = uuid!("a8f4c1e2-6b3d-4f5a-9c0d-1e2f3a4b5c6d");

#[derive(Debug, Clone)]
pub struct MediaTokenContext {
    pub run_id: String,
    pub conversation_id: String,
}

#[derive(Debug, Clone, Copy)]
pub enum MediaUnderstandKind {
    Image,
    Audio,
    Video,
    Pdf,
}

impl MediaUnderstandKind {
    pub fn role_id(self) -> &'static str {
        match self {
            Self::Image => "media-image-understand",
            Self::Audio => "media-audio-transcribe",
            Self::Video => "media-video-understand",
            Self::Pdf => "media-pdf-understand",
        }
    }

    pub fn instance_id(self, run_id: &str) -> String {
        let seed = format!("{}:{run_id}", self.role_id());
        Uuid::new_v5(&MEDIA_INSTANCE_NAMESPACE, seed.as_bytes()).to_string()
    }
}

pub fn media_understand_scope(
    ctx: &MediaTokenContext,
    kind: MediaUnderstandKind,
) -> AgentInstanceScope {
    AgentInstanceScope::with_instance_id(
        &ctx.run_id,
        &ctx.conversation_id,
        kind.role_id(),
        kind.instance_id(&ctx.run_id),
    )
}

pub fn record_media_understand_usage(
    ctx: &MediaTokenContext,
    kind: MediaUnderstandKind,
    out: &ChatOnceOutput,
) {
    let scope = media_understand_scope(ctx, kind);
    let model = model_name_for_usage_report(&out.model);
    match token_usage_store::record_round(&scope, out.usage.as_ref(), model, None) {
        Ok(()) => {
            if let Some(u) = out.usage.as_ref() {
                log::info!(
                    "media token usage {} kind={} model={} total={} prompt={} completion={}",
                    scope.log_suffix(),
                    kind.role_id(),
                    out.model,
                    u.total_tokens,
                    u.prompt_tokens,
                    u.completion_tokens
                );
            } else {
                log::warn!(
                    "media token usage missing usage payload {} kind={} model={}",
                    scope.log_suffix(),
                    kind.role_id(),
                    out.model
                );
            }
        }
        Err(e) => {
            log::warn!(
                "token_usage_store: media record_round failed {} kind={}: {e}",
                scope.log_suffix(),
                kind.role_id()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_instance_ids_are_uuid_and_distinct_per_kind() {
        let ctx = MediaTokenContext {
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        };
        let image = media_understand_scope(&ctx, MediaUnderstandKind::Image);
        assert!(Uuid::parse_str(&image.agent_instance_id).is_ok());
        assert_eq!(image.agent_role_id, "media-image-understand");
        let audio = media_understand_scope(&ctx, MediaUnderstandKind::Audio);
        assert!(Uuid::parse_str(&audio.agent_instance_id).is_ok());
        assert_ne!(image.agent_instance_id, audio.agent_instance_id);
        // request_id uses ':' delimiters — instance id must not contain extra colons.
        let request_id = token_usage_store::request_id_for_run(
            &ctx.run_id,
            &image.agent_instance_id,
            "qwen3.5-flash",
        );
        assert_eq!(request_id.matches(':').count(), 3);
    }
}
