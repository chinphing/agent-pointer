//! DashScope-native video understanding (qwen3.5-flash); falls back to None for ffmpeg path.

use crate::models::ModelSettings;
use crate::media::token::MediaTokenContext;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

/// Try native DashScope video understanding. Returns `Ok(None)` to use ffmpeg frame fallback.
pub async fn understand_video_dashscope(
    _settings: &ModelSettings,
    _api_key: &str,
    _bytes: &[u8],
    _file_name: &str,
    _token_ctx: &MediaTokenContext,
    _cancel: &CancellationToken,
) -> Result<Option<String>> {
    // Native DashScope file/video input TBD; dispatch uses ffmpeg frames + vision model.
    Ok(None)
}
