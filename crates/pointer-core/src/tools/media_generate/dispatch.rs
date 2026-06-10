//! Async execution for `image_generate` / `video_generate` tools.

use crate::chat_service::StreamTx;
use crate::media_generation::{
    format_generation_tool_result, generate_image, generate_video, GenerationKind,
};
use crate::models::ModelSettings;
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{build_image_request, build_video_request};

pub struct MediaGenerateDispatchContext<'a> {
    pub settings: &'a ModelSettings,
    pub conversation_id: &'a str,
    pub run_id: &'a str,
    pub tool_id: &'a str,
    pub args: Value,
    pub cancel: CancellationToken,
    pub stream: StreamTx,
    pub message_id: String,
    pub tool_call_id: String,
}

pub async fn dispatch_media_generate_async(
    ctx: MediaGenerateDispatchContext<'_>,
) -> Result<(String, bool, Option<String>)> {
    if let Some(ignored) = ctx
        .args
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        log::warn!(
            "media_generate: ignoring tool model arg \"{ignored}\" — use settings mediaModelOverrides"
        );
    }
    let result = match ctx.tool_id {
        "image_generate" => {
            let req = build_image_request(&ctx.args)?;
            generate_image(
                ctx.settings,
                ctx.conversation_id,
                ctx.run_id,
                &req,
                ctx.cancel,
            )
            .await
            .map(|a| (a, GenerationKind::Image))?
        }
        "video_generate" => {
            let req = build_video_request(&ctx.args)?;
            generate_video(
                ctx.settings,
                ctx.conversation_id,
                ctx.run_id,
                &req,
                ctx.cancel,
            )
            .await
            .map(|a| (a, GenerationKind::Video))?
        }
        other => return Err(anyhow!("unknown media tool {other}")),
    };
    let (artifact, kind) = result;
    let text = format_generation_tool_result(&artifact, kind);
    Ok((text, true, None))
}
