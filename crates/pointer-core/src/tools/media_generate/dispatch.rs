//! Async execution for `image_generate` / `video_generate` tools.

use crate::chat_service::StreamTx;
use crate::media_generation::{
    format_generation_tool_result, generate_image, generate_video,
    resolve_reference_image_for_api_in_env, GenerationKind,
};
use crate::models::ModelSettings;
use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{build_image_request, build_video_request};

/// 参考图可能是沙箱里的工作区路径（`/data/.../ref.png`）：按会话取**已建好的**
/// 环境，先把本地路径换成 `data:` URL（远端 API 只认 URL）。
///
/// http(s) / `data:` 原样返回；取不到会话环境时按宿主读（与以前一致）。
fn resolve_reference_image(image_url: &mut Option<String>, conversation_id: &str) -> Result<()> {
    let Some(raw) = image_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };
    let env = crate::env::session_env::env_for_conversation(conversation_id);
    *image_url = Some(resolve_reference_image_for_api_in_env(raw, env.as_deref())?);
    Ok(())
}

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
            let mut req = build_image_request(&ctx.args)?;
            resolve_reference_image(&mut req.image_url, ctx.conversation_id)?;
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
            let mut req = build_video_request(&ctx.args)?;
            resolve_reference_image(&mut req.image_url, ctx.conversation_id)?;
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
    let text = format_generation_tool_result(&artifact, kind, ctx.conversation_id);
    Ok((text, true, None))
}
