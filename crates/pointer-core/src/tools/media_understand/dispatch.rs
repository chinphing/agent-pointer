//! Async execution for `media_understand`.

use crate::chat_service::StreamTx;
use crate::media::apply::HARD_IMAGE_MAX_BYTES;
use crate::media::token::{MediaTokenContext, MediaUnderstandKind};
use crate::media::{
    describe_image_with_model, describe_pdf_pages_with_model, describe_video_with_model,
    extract_pdf_page_images_base64, extract_pdf_text_sorted, ffmpeg_available,
    extract_video_frame_base64s, focus_extracted_pdf_text_with_goal, read_media_ref_bytes,
    resolve_media_ref, transcribe_audio_with_model,
};
use crate::media::audio::{prepare_audio_bytes_for_asr_cached, AudioStorageContext};
use crate::media::store::{conversation_media_abs_to_rel, parse_conversation_media_ids};
use crate::mode_llm::resolve_media_mode_llm;
use crate::models::ModelSettings;
use crate::tools::media_understand::{format_goal_block, parse_context, parse_goal, parse_ref_and_mode};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde_json::Value;
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub struct MediaUnderstandDispatchContext<'a> {
    pub settings: &'a ModelSettings,
    pub conversation_id: &'a str,
    pub run_id: &'a str,
    pub args: Value,
    pub cancel: CancellationToken,
    pub stream: StreamTx,
    pub message_id: String,
    pub tool_call_id: String,
}

fn resolve_api_key(settings: &ModelSettings) -> Result<String> {
    let pid = settings.active_provider_id.trim();
    let key = settings
        .providers
        .iter()
        .find(|p| p.id == pid)
        .map(|p| p.api_key.trim())
        .unwrap_or("");
    if key.is_empty() {
        anyhow::bail!("no API key configured for media understanding");
    }
    Ok(key.to_string())
}

fn mime_from_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".into(),
        Some("jpg") | Some("jpeg") => "image/jpeg".into(),
        Some("gif") => "image/gif".into(),
        Some("webp") => "image/webp".into(),
        Some("pdf") => "application/pdf".into(),
        Some("mp4") | Some("m4v") => "video/mp4".into(),
        Some("webm") => "video/webm".into(),
        Some("mov") => "video/quicktime".into(),
        Some("mkv") => "video/x-matroska".into(),
        Some("mp3") => "audio/mpeg".into(),
        Some("wav") => "audio/wav".into(),
        Some("m4a") => "audio/mp4".into(),
        Some("aac") => "audio/aac".into(),
        Some("ogg") => "audio/ogg".into(),
        Some("flac") => "audio/flac".into(),
        _ => "application/octet-stream".into(),
    }
}

fn maybe_downscale_image_jpeg(bytes: &[u8], file_name: &str) -> Result<(Vec<u8>, String)> {
    if bytes.len() <= HARD_IMAGE_MAX_BYTES {
        return Ok((bytes.to_vec(), mime_from_path(Path::new(file_name))));
    }
    let img = image::load_from_memory(bytes).context("decode image for resize")?;
    let mut out = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut out);
    let rgb = img.to_rgb8();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 82);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .context("encode resized jpeg")?;
    if out.len() > HARD_IMAGE_MAX_BYTES {
        anyhow::bail!(
            "image {} still exceeds {} bytes after resize",
            file_name,
            HARD_IMAGE_MAX_BYTES
        );
    }
    Ok((out, "image/jpeg".into()))
}

async fn understand_image(
    settings: &ModelSettings,
    api_key: &str,
    bytes: &[u8],
    file_name: &str,
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let (prepared, mime) = maybe_downscale_image_jpeg(bytes, file_name)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&prepared);
    let image_model = resolve_media_mode_llm(settings, "image");
    describe_image_with_model(
        settings,
        &image_model,
        api_key,
        &b64,
        &mime,
        goal,
        token_ctx,
        cancel,
    )
    .await
}

async fn understand_audio(
    settings: &ModelSettings,
    api_key: &str,
    path: &Path,
    bytes: &[u8],
    file_name: &str,
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let mime = mime_from_path(path);
    let storage_ctx = conversation_media_abs_to_rel(path).map(|rel| {
        let (conv, id) = parse_conversation_media_ids(&rel).unwrap_or((String::new(), String::new()));
        AudioStorageContext {
            storage_rel_path: Some(rel),
            conversation_id: conv,
            attachment_id: id,
        }
    });
    let prepared = prepare_audio_bytes_for_asr_cached(&bytes, &mime, file_name, storage_ctx.as_ref())
        .context("prepare audio for ASR")?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&prepared.bytes);
    let audio_model = resolve_media_mode_llm(settings, "audio");
    transcribe_audio_with_model(
        settings,
        &audio_model,
        api_key,
        &b64,
        &prepared.mime_type,
        &prepared.file_name,
        goal,
        token_ctx,
        cancel,
    )
    .await
}

async fn understand_video(
    settings: &ModelSettings,
    api_key: &str,
    bytes: &[u8],
    file_name: &str,
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if let Some(text) = crate::media::dashscope_video::understand_video_dashscope(
        settings,
        api_key,
        bytes,
        file_name,
        token_ctx,
        cancel,
    )
    .await?
    {
        return Ok(text);
    }

    if !ffmpeg_available() {
        anyhow::bail!("video understanding requires ffmpeg or DashScope video support");
    }
    let frames = extract_video_frame_base64s(bytes, file_name).context("extract video frames")?;
    let video_model = resolve_media_mode_llm(settings, "video");
    describe_video_with_model(
        settings,
        &video_model,
        api_key,
        &frames,
        file_name,
        goal,
        token_ctx,
        cancel,
    )
    .await
}

async fn understand_pdf(
    settings: &ModelSettings,
    api_key: &str,
    bytes: &[u8],
    file_name: &str,
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let text_model = resolve_media_mode_llm(settings, "image");
    if let Ok(text) = extract_pdf_text_sorted(bytes, file_name) {
        return focus_extracted_pdf_text_with_goal(
            settings,
            &text_model,
            api_key,
            &text,
            file_name,
            goal,
            token_ctx,
            cancel,
        )
        .await;
    }
    log::info!(
        "media_understand pdf {file_name}: sorted text insufficient; OCR fallback"
    );
    let pages = extract_pdf_page_images_base64(bytes, file_name).context("pdf page raster")?;
    let image_model = resolve_media_mode_llm(settings, "image");
    describe_pdf_pages_with_model(
        settings,
        &image_model,
        api_key,
        &pages,
        file_name,
        goal,
        token_ctx,
        cancel,
    )
    .await
}

pub async fn dispatch_media_understand_async(
    ctx: MediaUnderstandDispatchContext<'_>,
) -> Result<(String, bool, Option<String>)> {
    let (media_ref, mode) = parse_ref_and_mode(&ctx.args)?;
    let goal = parse_goal(&ctx.args)?;
    let context = parse_context(&ctx.args);
    let goal_block = format_goal_block(&goal, context.as_deref());
    let path = resolve_media_ref(&media_ref).with_context(|| format!("resolve media ref {media_ref}"))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let bytes = read_media_ref_bytes(&media_ref)?;
    let api_key = resolve_api_key(ctx.settings)?;
    let token_ctx = MediaTokenContext {
        run_id: ctx.run_id.to_string(),
        conversation_id: ctx.conversation_id.to_string(),
    };

    let text = match mode.as_str() {
        "image" => {
            let _ = MediaUnderstandKind::Image;
            understand_image(
                ctx.settings,
                &api_key,
                &bytes,
                &file_name,
                &goal_block,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        "audio" => {
            understand_audio(
                ctx.settings,
                &api_key,
                &path,
                &bytes,
                &file_name,
                &goal_block,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        "video" => {
            understand_video(
                ctx.settings,
                &api_key,
                &bytes,
                &file_name,
                &goal_block,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        "pdf" => {
            understand_pdf(
                ctx.settings,
                &api_key,
                &bytes,
                &file_name,
                &goal_block,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        other => return Err(anyhow!("unsupported media_understand mode: {other}")),
    };

    Ok((text, true, None))
}
