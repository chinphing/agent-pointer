//! Async execution for `media_understand`.

use crate::chat_service::StreamTx;
use crate::media::jpeg_vision::prepare_jpeg_for_vision;
use crate::media::token::{MediaTokenContext, MediaUnderstandKind};
use crate::media::{
    describe_image_with_model, describe_images_with_model, describe_pdf_pages_with_model,
    describe_video_with_model, download_video_from_url, extract_pdf_page_images_base64_range,
    ffmpeg_available, extract_video_frame_base64s_with_range, find_attachment_by_media_ref,
    format_image_dir_scope_notice, format_pdf_scope_notice, format_video_scope_notice,
    list_image_files_in_dir, pdf_page_count, prepare_video_bytes_for_range, probe_video_duration,
    read_media_ref_bytes, resolve_media_ref, transcribe_audio_with_model, is_video_file_name,
};
use crate::media::audio::{prepare_audio_bytes_for_asr_cached, AudioStorageContext};
use crate::media::store::{conversation_media_abs_to_rel, parse_conversation_media_ids};
use crate::mode_llm::resolve_media_mode_llm;
use crate::models::ModelSettings;
use crate::tools::media_understand::{
    format_goal_block, parse_context, parse_goal, parse_image_dir_range, parse_pdf_page_range,
    parse_ref_and_mode, parse_video_time_range, prepend_scope_notice,
};
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
    let prepared = prepare_jpeg_for_vision(bytes, file_name)?;
    Ok((prepared, "image/jpeg".into()))
}

async fn understand_image_file(
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

async fn understand_image_directory(
    settings: &ModelSettings,
    api_key: &str,
    dir: &Path,
    goal: &str,
    args: &Value,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let files = list_image_files_in_dir(dir).context("list images in directory")?;
    let total = files.len();
    let range = parse_image_dir_range(args, total)?;
    let notice = format_image_dir_scope_notice(&range, total, &dir.display().to_string());

    let mut images_base64 = Vec::new();
    let mut labels = Vec::new();
    for (ordinal, path) in files.into_iter().enumerate() {
        let index = ordinal + 1;
        if index < range.start || index > range.end {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string();
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read image {}", path.display()))?;
        let (prepared, _mime) = maybe_downscale_image_jpeg(&bytes, &file_name)?;
        images_base64.push(base64::engine::general_purpose::STANDARD.encode(&prepared));
        labels.push(file_name);
    }
    if images_base64.is_empty() {
        anyhow::bail!("no images loaded for directory range {}-{}", range.start, range.end);
    }

    let image_model = resolve_media_mode_llm(settings, "image");
    let body = describe_images_with_model(
        settings,
        &image_model,
        api_key,
        &images_base64,
        &labels,
        &dir.display().to_string(),
        goal,
        token_ctx,
        cancel,
    )
    .await?;
    Ok(prepend_scope_notice(&body, &notice))
}

async fn understand_image(
    settings: &ModelSettings,
    api_key: &str,
    path: &Path,
    goal: &str,
    args: &Value,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if path.is_dir() {
        return understand_image_directory(settings, api_key, path, goal, args, token_ctx, cancel).await;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let bytes = std::fs::read(path).with_context(|| format!("read image {}", path.display()))?;
    understand_image_file(
        settings,
        api_key,
        &bytes,
        &file_name,
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
    bytes: Option<&[u8]>,
    remote_url: Option<&str>,
    file_name: &str,
    goal: &str,
    args: &Value,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let video_model = resolve_media_mode_llm(settings, "video");
    let duration = if let Some(b) = bytes {
        probe_video_duration(b, file_name).context("probe video duration")?
    } else {
        3600.0
    };
    let range = parse_video_time_range(args, duration)?;
    let scope_notice = |native: bool, native_via_oss: bool, frame_count: usize| {
        format_video_scope_notice(&range, duration, frame_count, native, native_via_oss)
    };

    if let Some(url) = remote_url.filter(|s| !s.trim().is_empty()) {
        log::info!("media_understand video {file_name}: native video_url via OSS remoteUrl");
        match crate::media::dashscope_video::understand_video_dashscope_url(
            settings,
            &video_model,
            api_key,
            url,
            bytes.map(|b| b.len()).unwrap_or(0),
            "OSS attachment",
            goal,
            range.frames_per_second,
            token_ctx,
            cancel,
        )
        .await
        {
            Ok(Some(text)) => {
                return Ok(prepend_scope_notice(&text, &scope_notice(true, true, 0)));
            }
            Ok(None) => {
                log::info!(
                    "media_understand video {file_name}: DashScope OSS URL unavailable; fallback"
                );
            }
            Err(e) => {
                log::warn!(
                    "media_understand video {file_name}: OSS URL native failed ({e:#}); fallback"
                );
            }
        }
    }

    let bytes = if let Some(b) = bytes {
        b.to_vec()
    } else if let Some(url) = remote_url {
        download_video_from_url(url, cancel)
            .await
            .context("download video from remoteUrl")?
    } else {
        anyhow::bail!("video requires remoteUrl or local file bytes");
    };

    let clip_bytes =
        prepare_video_bytes_for_range(&bytes, file_name, &range, duration).context("prepare video clip")?;

    if !ffmpeg_available() {
        anyhow::bail!("video understanding requires DashScope-compatible provider with OSS URL or ffmpeg");
    }
    let (frames, meta) = extract_video_frame_base64s_with_range(&clip_bytes, file_name, &range)
        .context("extract video frames")?;
    let body = describe_video_with_model(
        settings,
        &video_model,
        api_key,
        &frames,
        file_name,
        goal,
        token_ctx,
        cancel,
    )
    .await?;
    Ok(prepend_scope_notice(
        &body,
        &scope_notice(false, false, meta.frame_count),
    ))
}

async fn understand_pdf(
    settings: &ModelSettings,
    api_key: &str,
    bytes: &[u8],
    file_name: &str,
    goal: &str,
    page_range: &crate::media::PdfPageRange,
    total_pages: usize,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    let notice = format_pdf_scope_notice(page_range, total_pages);
    log::info!(
        "media_understand pdf {file_name}: scanned PDF page-image vision (pages {}-{})",
        page_range.start,
        page_range.end
    );
    // Pdfium init may download the native library via reqwest::blocking; must not run on tokio workers.
    let pdf_bytes = bytes.to_vec();
    let pdf_name = file_name.to_string();
    let range = *page_range;
    let pages = tokio::task::spawn_blocking(move || {
        extract_pdf_page_images_base64_range(&pdf_bytes, &pdf_name, &range)
    })
    .await
    .context("pdf page render task failed")?
    .context("pdf page render (pdfium)")?;
    let image_model = resolve_media_mode_llm(settings, "image");
    let body = describe_pdf_pages_with_model(
        settings,
        &image_model,
        api_key,
        &pages,
        file_name,
        goal,
        token_ctx,
        cancel,
    )
    .await?;
    Ok(prepend_scope_notice(&body, &notice))
}

pub async fn dispatch_media_understand_async(
    ctx: MediaUnderstandDispatchContext<'_>,
) -> Result<(String, bool, Option<String>)> {
    let (media_ref, mode) = parse_ref_and_mode(&ctx.args)?;
    let goal = parse_goal(&ctx.args)?;
    let context = parse_context(&ctx.args);
    let goal_block = format_goal_block(&goal, context.as_deref());
    let attachment = find_attachment_by_media_ref(ctx.conversation_id, &media_ref).ok().flatten();
    let remote_url = attachment
        .as_ref()
        .and_then(|a| a.remote_url.as_deref())
        .filter(|s| !s.trim().is_empty());
    let file_name = attachment
        .as_ref()
        .map(|a| a.file_name.clone())
        .unwrap_or_else(|| {
            std::path::Path::new(media_ref.trim())
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("attachment")
                .to_string()
        });
    let path = if mode == "video" && remote_url.is_some() {
        resolve_media_ref(&media_ref).unwrap_or_else(|_| std::path::PathBuf::from(&file_name))
    } else {
        resolve_media_ref(&media_ref)
            .with_context(|| format!("resolve media ref {media_ref}"))?
    };
    let bytes = if path.is_file() {
        Some(
            read_media_ref_bytes(&media_ref)
                .with_context(|| format!("read media bytes for {media_ref}"))?,
        )
    } else {
        None
    };
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
                &path,
                &goal_block,
                &ctx.args,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        "audio" => {
            let bytes = bytes.as_deref().ok_or_else(|| anyhow!("audio ref must be a file"))?;
            if attachment
                .as_ref()
                .is_some_and(|a| a.kind == "video")
                || is_video_file_name(&file_name)
            {
                log::info!(
                    "media_understand: mode=audio on video file {file_name}; extracting audio track for ASR"
                );
            }
            understand_audio(
                ctx.settings,
                &api_key,
                &path,
                bytes,
                &file_name,
                &goal_block,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        "video" => {
            if bytes.is_none() && remote_url.is_none() {
                anyhow::bail!("video ref has no local file or remoteUrl");
            }
            understand_video(
                ctx.settings,
                &api_key,
                bytes.as_deref(),
                remote_url,
                &file_name,
                &goal_block,
                &ctx.args,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        "pdf" => {
            let bytes = bytes.as_deref().ok_or_else(|| anyhow!("pdf ref must be a file"))?;
            let total_pages = pdf_page_count(&bytes, &file_name)
                .with_context(|| format!("read pdf page count for {file_name}"))?;
            let page_range = parse_pdf_page_range(&ctx.args, total_pages)?;
            understand_pdf(
                ctx.settings,
                &api_key,
                &bytes,
                &file_name,
                &goal_block,
                &page_range,
                total_pages,
                &token_ctx,
                &ctx.cancel,
            )
            .await?
        }
        other => return Err(anyhow!("unsupported media_understand mode: {other}")),
    };

    Ok((text, true, None))
}
