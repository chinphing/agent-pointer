//! Async execution for `media_understand`.

use crate::chat_service::StreamTx;
use crate::media::audio::{prepare_audio_bytes_for_asr_cached, AudioStorageContext};
use crate::media::jpeg_vision::prepare_jpeg_for_vision;
use crate::media::manifest::{attachment_local_abs_path, attachment_ref_uri};
use crate::media::store::{conversation_media_abs_to_rel, parse_conversation_media_ids};
use crate::media::token::MediaTokenContext;
use crate::media::{
    conversation_user_attachments, describe_image_with_model, describe_images_with_model,
    describe_pdf_pages_with_model, describe_video_with_model, download_video_from_url,
    extract_video_frame_base64s_with_range, ffmpeg_available, find_attachment_by_id,
    find_attachment_by_media_ref, format_image_dir_scope_notice, format_multi_refs_scope_notice,
    format_pdf_scope_notice, format_video_scope_notice, is_video_file_name,
    list_image_files_in_dir, prepare_video_bytes_for_range, probe_video_duration,
    read_media_ref_bytes, resolve_media_ref, transcribe_audio_with_model,
};
use crate::mode_llm::resolve_media_mode_llm;
use crate::models::ModelSettings;
use crate::tools::media_understand::{
    format_goal_block, parse_context, parse_goal, parse_image_dir_range, parse_mode,
    parse_pdf_page_range, parse_ref_inputs, parse_video_time_range, prepend_scope_notice,
    MediaRefInput,
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

fn attachment_media_ref(att: &crate::models::MediaAttachment) -> Option<String> {
    att.storage_rel_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(attachment_ref_uri)
        .or_else(|| attachment_local_abs_path(att))
}

fn attachment_candidates(conversation_id: &str, raw: &str) -> String {
    let needle_name = Path::new(raw.trim())
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(raw.trim())
        .to_ascii_lowercase();
    let Ok(mut attachments) = conversation_user_attachments(conversation_id) else {
        return String::new();
    };
    attachments.sort_by_key(|att| {
        let name = att.file_name.to_ascii_lowercase();
        if name == needle_name {
            0
        } else if raw.contains(&att.id) || name.contains(&needle_name) {
            1
        } else {
            2
        }
    });
    attachments.dedup_by(|a, b| a.id == b.id);
    let candidates: Vec<String> = attachments
        .into_iter()
        .filter_map(|att| {
            attachment_media_ref(&att).map(|media_ref| {
                format!(
                    "attachmentId={} fileName={} ref={media_ref}",
                    att.id, att.file_name
                )
            })
        })
        .take(3)
        .collect();
    if candidates.is_empty() {
        String::new()
    } else {
        format!(
            "\nCurrent-conversation attachment candidates:\n- {}",
            candidates.join("\n- ")
        )
    }
}

fn resolve_ref_inputs(conversation_id: &str, args: &Value, mode: &str) -> Result<Vec<String>> {
    parse_ref_inputs(args, mode)?
        .into_iter()
        .map(|input| match input {
            MediaRefInput::AttachmentId(id) => {
                let attachment = find_attachment_by_id(conversation_id, &id)?.ok_or_else(|| {
                    anyhow!(
                        "attachmentId not found in current conversation: {id}{}",
                        attachment_candidates(conversation_id, &id)
                    )
                })?;
                attachment_media_ref(&attachment).ok_or_else(|| {
                    anyhow!("attachmentId {id} has no resolvable local attachment ref")
                })
            }
            MediaRefInput::Ref(raw) => {
                if resolve_media_ref(&raw).is_ok()
                    || find_attachment_by_media_ref(conversation_id, &raw)?.is_some()
                {
                    Ok(raw)
                } else {
                    Err(anyhow!(
                        "media ref not found: {raw}{}",
                        attachment_candidates(conversation_id, &raw)
                    ))
                }
            }
        })
        .collect()
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
        let bytes =
            std::fs::read(&path).with_context(|| format!("read image {}", path.display()))?;
        let (prepared, _mime) = maybe_downscale_image_jpeg(&bytes, &file_name)?;
        images_base64.push(base64::engine::general_purpose::STANDARD.encode(&prepared));
        labels.push(file_name);
    }
    if images_base64.is_empty() {
        anyhow::bail!(
            "no images loaded for directory range {}-{}",
            range.start,
            range.end
        );
    }

    let image_model = resolve_media_mode_llm(settings, "image");
    let body = describe_images_with_model(
        settings,
        &image_model,
        api_key,
        &images_base64,
        &labels,
        &format!("from directory \"{}\"", dir.display()),
        goal,
        token_ctx,
        cancel,
    )
    .await?;
    Ok(prepend_scope_notice(&body, &notice))
}

async fn understand_image_refs(
    settings: &ModelSettings,
    api_key: &str,
    refs: &[String],
    goal: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    if refs.is_empty() {
        anyhow::bail!("no image refs to understand");
    }
    if refs.len() == 1 {
        let media_ref = &refs[0];
        let path = resolve_media_ref(media_ref)
            .with_context(|| format!("resolve media ref {media_ref}"))?;
        if path.is_dir() {
            anyhow::bail!(
                "image ref is a directory; pass refs with one directory path and use pageStart/pageEnd"
            );
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let bytes = read_media_ref_bytes(media_ref)
            .with_context(|| format!("read media bytes for {media_ref}"))?;
        return understand_image_file(
            settings, api_key, &bytes, &file_name, goal, token_ctx, cancel,
        )
        .await;
    }

    let mut images_base64 = Vec::with_capacity(refs.len());
    let mut labels = Vec::with_capacity(refs.len());
    for media_ref in refs {
        let path = resolve_media_ref(media_ref)
            .with_context(|| format!("resolve media ref {media_ref}"))?;
        if path.is_dir() {
            anyhow::bail!(
                "refs entry {media_ref} is a directory; pass one directory path in refs with pageStart/pageEnd"
            );
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let bytes = read_media_ref_bytes(media_ref)
            .with_context(|| format!("read media bytes for {media_ref}"))?;
        let (prepared, _mime) = maybe_downscale_image_jpeg(&bytes, &file_name)?;
        images_base64.push(base64::engine::general_purpose::STANDARD.encode(&prepared));
        labels.push(file_name);
    }

    let notice = format_multi_refs_scope_notice(refs.len());
    let image_model = resolve_media_mode_llm(settings, "image");
    let body = describe_images_with_model(
        settings,
        &image_model,
        api_key,
        &images_base64,
        &labels,
        "from the attached batch",
        goal,
        token_ctx,
        cancel,
    )
    .await?;
    Ok(prepend_scope_notice(&body, &notice))
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
        let (conv, id) =
            parse_conversation_media_ids(&rel).unwrap_or((String::new(), String::new()));
        AudioStorageContext {
            storage_rel_path: Some(rel),
            conversation_id: conv,
            attachment_id: id,
        }
    });
    let prepared =
        prepare_audio_bytes_for_asr_cached(&bytes, &mime, file_name, storage_ctx.as_ref())
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

    let clip_bytes = prepare_video_bytes_for_range(&bytes, file_name, &range, duration)
        .context("prepare video clip")?;

    if !ffmpeg_available() {
        anyhow::bail!(
            "video understanding requires DashScope-compatible provider with OSS URL or ffmpeg"
        );
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
    page_args: serde_json::Value,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<String> {
    // Pdfium is a single process-wide engine; page count + render must run on one thread.
    let pdf_bytes = bytes.to_vec();
    let pdf_name = file_name.to_string();
    let (pages, page_range, total_pages) = tokio::task::spawn_blocking(move || {
        use crate::media::pdf::{extract_pdf_page_images_base64_range, pdf_page_count};

        let total_pages = pdf_page_count(&pdf_bytes, &pdf_name)
            .with_context(|| format!("read pdf page count for {pdf_name}"))?;
        let page_range = parse_pdf_page_range(&page_args, total_pages)?;
        let pages = extract_pdf_page_images_base64_range(&pdf_bytes, &pdf_name, &page_range)?;
        Ok::<_, anyhow::Error>((pages, page_range, total_pages))
    })
    .await
    .context("pdf page render task failed")??;

    let notice = format_pdf_scope_notice(&page_range, total_pages);
    log::info!(
        "media_understand pdf {file_name}: scanned PDF page-image vision (pages {}-{})",
        page_range.start,
        page_range.end
    );
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
    let mode = parse_mode(&ctx.args)?;
    let goal = parse_goal(&ctx.args)?;
    let context = parse_context(&ctx.args);
    let goal_block = format_goal_block(&goal, context.as_deref());
    let api_key = resolve_api_key(ctx.settings)?;
    let token_ctx = MediaTokenContext {
        run_id: ctx.run_id.to_string(),
        conversation_id: ctx.conversation_id.to_string(),
    };

    let resolved_refs = resolve_ref_inputs(ctx.conversation_id, &ctx.args, mode.as_str())?;
    let text = match mode.as_str() {
        "image" => {
            let refs = resolved_refs;
            let goal_preview = crate::text_util::take_chars(&goal, 80);
            log::info!(
                "dispatch_media_understand_async mode=image refs={refs:?} goal={goal_preview}"
            );
            if refs.len() == 1 {
                let path = resolve_media_ref(&refs[0])
                    .with_context(|| format!("resolve media ref {}", refs[0]))?;
                if path.is_dir() {
                    understand_image_directory(
                        ctx.settings,
                        &api_key,
                        &path,
                        &goal_block,
                        &ctx.args,
                        &token_ctx,
                        &ctx.cancel,
                    )
                    .await?
                } else {
                    understand_image_refs(
                        ctx.settings,
                        &api_key,
                        &refs,
                        &goal_block,
                        &token_ctx,
                        &ctx.cancel,
                    )
                    .await?
                }
            } else {
                understand_image_refs(
                    ctx.settings,
                    &api_key,
                    &refs,
                    &goal_block,
                    &token_ctx,
                    &ctx.cancel,
                )
                .await?
            }
        }
        "audio" | "video" | "pdf" => {
            let media_ref = resolved_refs
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("missing refs"))?;
            dispatch_single_ref_media(
                ctx,
                mode.as_str(),
                &media_ref,
                &goal_block,
                &api_key,
                &token_ctx,
            )
            .await?
        }
        other => return Err(anyhow!("unsupported media_understand mode: {other}")),
    };

    Ok((text, true, None))
}

async fn dispatch_single_ref_media(
    ctx: MediaUnderstandDispatchContext<'_>,
    mode: &str,
    media_ref: &str,
    goal_block: &str,
    api_key: &str,
    token_ctx: &MediaTokenContext,
) -> Result<String> {
    let attachment = find_attachment_by_media_ref(ctx.conversation_id, media_ref)
        .ok()
        .flatten();
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
        resolve_media_ref(media_ref).unwrap_or_else(|_| std::path::PathBuf::from(&file_name))
    } else {
        resolve_media_ref(media_ref).with_context(|| format!("resolve media ref {media_ref}"))?
    };
    let bytes = if path.is_file() {
        Some(
            read_media_ref_bytes(media_ref)
                .with_context(|| format!("read media bytes for {media_ref}"))?,
        )
    } else {
        None
    };

    match mode {
        "audio" => {
            let bytes = bytes
                .as_deref()
                .ok_or_else(|| anyhow!("audio ref must be a file"))?;
            if attachment.as_ref().is_some_and(|a| a.kind == "video")
                || is_video_file_name(&file_name)
            {
                log::info!(
                    "media_understand: mode=audio on video file {file_name}; extracting audio track for ASR"
                );
            }
            understand_audio(
                ctx.settings,
                api_key,
                &path,
                bytes,
                &file_name,
                goal_block,
                token_ctx,
                &ctx.cancel,
            )
            .await
        }
        "video" => {
            if bytes.is_none() && remote_url.is_none() {
                anyhow::bail!("video ref has no local file or remoteUrl");
            }
            understand_video(
                ctx.settings,
                api_key,
                bytes.as_deref(),
                remote_url,
                &file_name,
                goal_block,
                &ctx.args,
                token_ctx,
                &ctx.cancel,
            )
            .await
        }
        "pdf" => {
            let bytes = bytes
                .as_deref()
                .ok_or_else(|| anyhow!("pdf ref must be a file"))?;
            understand_pdf(
                ctx.settings,
                api_key,
                bytes,
                &file_name,
                goal_block,
                ctx.args.clone(),
                token_ctx,
                &ctx.cancel,
            )
            .await
        }
        other => Err(anyhow!("unsupported media_understand mode: {other}")),
    }
}
