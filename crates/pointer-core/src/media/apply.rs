use crate::media::capabilities::model_supports_vision;
use crate::mode_llm::resolve_media_mode_llm;
use crate::media::retry::{
    auto_ffmpeg_video_retry_plan, content_has_media_block_for_file, detect_media_retry_plan,
    message_indices_for_retry, replace_media_injection, should_retry_attachment, MediaRetryPlan,
};
use crate::media::path_hint::append_attachment_paths;
use crate::media::store::{read_media_bytes, save_attachment_bytes};
use crate::media::token::MediaTokenContext;
use crate::media::understand::{
    describe_image_with_model, describe_pdf_pages_with_model, describe_video_with_model,
    transcribe_audio_with_model,
};
use crate::models::{AgentModelRef, ChatMessage, MediaAttachment, ModelSettings, Role};
use anyhow::{Context, Result};
use base64::Engine;
use tokio_util::sync::CancellationToken;

pub const INLINE_IMAGE_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const HARD_IMAGE_MAX_BYTES: usize = 6 * 1024 * 1024;
const MAX_DOCUMENT_TEXT_BYTES: usize = 256 * 1024;

fn with_saved_paths(att: &MediaAttachment, block: impl Into<String>) -> String {
    append_attachment_paths(&block.into(), att.storage_rel_path.as_deref())
}

fn is_image_mime(mime: &str) -> bool {
    mime.trim().to_ascii_lowercase().starts_with("image/")
}

fn is_text_document_mime(mime: &str) -> bool {
    let m = mime.trim().to_ascii_lowercase();
    m.starts_with("text/")
        || m == "application/json"
        || m == "application/xml"
        || m.ends_with("+json")
        || m.ends_with("+xml")
}

fn is_audio_mime(mime: &str) -> bool {
    mime.trim().to_ascii_lowercase().starts_with("audio/")
}

fn should_save_playable_wav(
    att: &MediaAttachment,
    prepared: &crate::media::audio::PreparedAudio,
) -> bool {
    if prepared.mime_type != "audio/wav" {
        return false;
    }
    let name = att.file_name.to_ascii_lowercase();
    let mime = att.mime_type.to_ascii_lowercase();
    let storage_bin = att
        .storage_rel_path
        .as_deref()
        .map(|p| p.to_ascii_lowercase().ends_with(".bin"))
        .unwrap_or(false);
    name.ends_with(".bin") || mime.contains("octet-stream") || storage_bin
}

fn is_video_mime(mime: &str) -> bool {
    mime.trim().to_ascii_lowercase().starts_with("video/")
}

fn effective_audio_model(settings: &ModelSettings) -> AgentModelRef {
    resolve_media_mode_llm(settings, "audio")
}

fn effective_video_model(settings: &ModelSettings) -> AgentModelRef {
    resolve_media_mode_llm(settings, "video")
}

fn effective_image_model(settings: &ModelSettings) -> AgentModelRef {
    resolve_media_mode_llm(settings, "image")
}

fn load_attachment_bytes(att: &MediaAttachment) -> Result<Vec<u8>> {
    if let Some(raw) = att
        .content_base64
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return base64::engine::general_purpose::STANDARD
            .decode(raw.trim())
            .context("decode attachment base64");
    }
    if let Some(rel) = att
        .storage_rel_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        return read_media_bytes(rel).context("read attachment from storage");
    }
    anyhow::bail!("attachment has no bytes source (contentBase64 or storageRelPath)")
}

fn extract_document_text(bytes: &[u8], file_name: &str) -> Result<String> {
    if bytes.len() > MAX_DOCUMENT_TEXT_BYTES {
        log::warn!(
            "document {} exceeds {} bytes; truncating for LLM",
            file_name,
            MAX_DOCUMENT_TEXT_BYTES
        );
    }
    let slice = &bytes[..bytes.len().min(MAX_DOCUMENT_TEXT_BYTES)];
    String::from_utf8(slice.to_vec()).context("document is not valid UTF-8 text")
}

async fn process_pdf_attachment(
    settings: &ModelSettings,
    att: &mut MediaAttachment,
    bytes: &[u8],
    api_key: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<(Option<String>, Option<String>)> {
    if let Ok(text) = crate::media::pdf::extract_pdf_text(bytes, &att.file_name) {
        att.derived_text = Some(text.clone());
        return Ok((
            None,
            Some(with_saved_paths(
                att,
                format!("[PDF: {}]\n```\n{}\n```", att.file_name, text),
            )),
        ));
    }

    log::info!(
        "media: pdf {} text insufficient (< {} chars); trying embedded page rasters",
        att.file_name,
        crate::media::pdf::MIN_PDF_TEXT_CHARS
    );
    match crate::media::pdf::extract_pdf_page_images_base64(bytes, &att.file_name) {
        Ok(pages) => {
            let image_model = effective_image_model(settings);
            log::info!(
                "media: describing scanned pdf {} ({} page images) via model {}",
                att.file_name,
                pages.len(),
                image_model.model
            );
            match describe_pdf_pages_with_model(
                settings,
                &image_model,
                api_key,
                &pages,
                &att.file_name,
                token_ctx,
                cancel,
            )
            .await
            {
                Ok(desc) => {
                    att.derived_text = Some(desc.clone());
                    Ok((
                        None,
                        Some(with_saved_paths(
                            att,
                            format!("[PDF (scanned): {}]\n{}", att.file_name, desc),
                        )),
                    ))
                }
                Err(e) => {
                    log::warn!(
                        "media: scanned pdf understanding failed for {}: {:#}",
                        att.file_name,
                        e
                    );
                    Ok((
                        None,
                        Some(crate::media::deps_hint::pdf_processing_failed_hint(
                            &att.file_name,
                            &format!("扫描页理解失败：{e}"),
                            att.storage_rel_path.as_deref(),
                        )),
                    ))
                }
            }
        }
        Err(e) => {
            log::warn!(
                "media: pdf raster extraction failed for {}: {:#}",
                att.file_name,
                e
            );
            Ok((
                None,
                Some(crate::media::deps_hint::pdf_processing_failed_hint(
                    &att.file_name,
                    &format!("无法提取文本或内嵌页图：{e}"),
                    att.storage_rel_path.as_deref(),
                )),
            ))
        }
    }
}

async fn process_attachment(
    settings: &ModelSettings,
    conversation_id: &str,
    att: &mut MediaAttachment,
    primary_vision: bool,
    api_key: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<(Option<String>, Option<String>)> {
    let bytes = load_attachment_bytes(att)?;
    process_attachment_with_bytes(
        settings,
        conversation_id,
        att,
        &bytes,
        primary_vision,
        api_key,
        token_ctx,
        cancel,
    )
    .await
}

async fn process_attachment_with_bytes(
    settings: &ModelSettings,
    conversation_id: &str,
    att: &mut MediaAttachment,
    bytes: &[u8],
    primary_vision: bool,
    api_key: &str,
    token_ctx: &MediaTokenContext,
    cancel: &CancellationToken,
) -> Result<(Option<String>, Option<String>)> {
    if att.storage_rel_path.as_deref().unwrap_or("").is_empty() {
        let rel = save_attachment_bytes(conversation_id, &att.id, &bytes, &att.file_name)?;
        att.storage_rel_path = Some(rel);
    }
    att.content_base64 = None;
    att.size_bytes = bytes.len() as u64;

    let mime = att.mime_type.clone();
    if is_image_mime(&mime) || att.kind == "image" {
        let image_mime = if is_image_mime(&mime) {
            mime.clone()
        } else {
            log::warn!(
                "media: image {} has non-image mime {}; treating as image/jpeg",
                att.file_name,
                mime
            );
            "image/jpeg".into()
        };
        if bytes.len() > HARD_IMAGE_MAX_BYTES {
            let limit_mb = HARD_IMAGE_MAX_BYTES / (1024 * 1024);
            log::warn!(
                "media: image {} exceeds {} MB limit",
                att.file_name,
                limit_mb
            );
            return Ok((
                None,
                Some(with_saved_paths(
                    att,
                    format!(
                        "[Image: {}] 超过 {limit_mb} MB 上限，未送入模型。请压缩后重发。",
                        att.file_name
                    ),
                )),
            ));
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        if primary_vision && bytes.len() <= INLINE_IMAGE_MAX_BYTES {
            return Ok((Some(b64), None));
        }
        let image_model = effective_image_model(settings);
        log::info!(
            "media: describing image {} via model {} (primary_vision={})",
            att.file_name,
            image_model.model,
            primary_vision
        );
        match describe_image_with_model(
            settings,
            &image_model,
            api_key,
            &b64,
            &image_mime,
            token_ctx,
            cancel,
        )
        .await
        {
            Ok(desc) => {
                att.derived_text = Some(desc.clone());
                return Ok((
                    None,
                    Some(with_saved_paths(
                        att,
                        format!("[Image: {}]\n{}", att.file_name, desc),
                    )),
                ));
            }
            Err(e) => {
                log::warn!(
                    "media: image understanding failed for {}: {:#}",
                    att.file_name,
                    e
                );
                return Ok((
                    None,
                    Some(crate::media::deps_hint::image_understanding_failed_hint(
                        &att.file_name,
                        &e.to_string(),
                        att.storage_rel_path.as_deref(),
                    )),
                ));
            }
        }
    }

    if is_audio_mime(&mime) || att.kind == "audio" {
        if let Some(existing) = att.derived_text.as_ref().filter(|t| !t.trim().is_empty()) {
            att.content_base64 = None;
            return Ok((
                None,
                Some(with_saved_paths(
                    att,
                    format!("[Audio: {}]\n{}", att.file_name, existing),
                )),
            ));
        }
        let prepared = match crate::media::audio::prepare_audio_bytes_for_asr(&bytes, &mime, &att.file_name)
        {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "media: audio prepare failed for {}: {:#}",
                    att.file_name,
                    e
                );
                return Ok((
                    None,
                    Some(crate::media::deps_hint::audio_transcription_failed_hint(
                        &att.file_name,
                        &e.to_string(),
                        att.storage_rel_path.as_deref(),
                    )),
                ));
            }
        };
        if should_save_playable_wav(att, &prepared) {
            match save_attachment_bytes(
                conversation_id,
                &att.id,
                &prepared.bytes,
                &prepared.file_name,
            ) {
                Ok(rel) => {
                    att.storage_rel_path = Some(rel);
                    att.mime_type = prepared.mime_type.clone();
                    att.file_name = prepared.file_name.clone();
                }
                Err(e) => {
                    log::warn!(
                        "media: failed to save playable wav for {}: {:#}",
                        att.file_name,
                        e
                    );
                }
            }
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&prepared.bytes);
        let audio_model = effective_audio_model(settings);
        log::info!(
            "media: transcribing audio {} via model {}",
            prepared.file_name,
            audio_model.model
        );
        match transcribe_audio_with_model(
            settings,
            &audio_model,
            api_key,
            &b64,
            &prepared.mime_type,
            &prepared.file_name,
            token_ctx,
            cancel,
        )
        .await
        {
            Ok(transcript) => {
                att.derived_text = Some(transcript.clone());
                return Ok((
                    None,
                    Some(with_saved_paths(
                        att,
                        format!("[Audio: {}]\n{}", att.file_name, transcript),
                    )),
                ));
            }
            Err(e) => {
                log::warn!(
                    "media: audio transcription failed for {}: {:#}",
                    att.file_name,
                    e
                );
                return Ok((
                    None,
                    Some(crate::media::deps_hint::audio_transcription_failed_hint(
                        &att.file_name,
                        &e.to_string(),
                        att.storage_rel_path.as_deref(),
                    )),
                ));
            }
        }
    }

    if is_text_document_mime(&mime) || att.kind == "document" {
        if mime == "application/pdf" || att.file_name.to_ascii_lowercase().ends_with(".pdf") {
            return process_pdf_attachment(settings, att, &bytes, api_key, token_ctx, cancel).await;
        }
        let text = extract_document_text(&bytes, &att.file_name)?;
        att.derived_text = Some(text.clone());
        return Ok((
            None,
            Some(with_saved_paths(
                att,
                format!("[File: {}]\n```\n{}\n```", att.file_name, text),
            )),
        ));
    }

    if is_video_mime(&mime) || att.kind == "video" {
        if !crate::media::ffmpeg::ffmpeg_available() {
            log::warn!(
                "media: ffmpeg unavailable; skip video processing for {}",
                att.file_name
            );
            return Ok((
                None,
                Some(crate::media::deps_hint::video_ffmpeg_missing(
                    &att.file_name,
                    att.storage_rel_path.as_deref(),
                )),
            ));
        }
        log::info!(
            "media: processing video {} via ffmpeg + video model",
            att.file_name
        );
        match crate::media::video::extract_video_frame_base64s(&bytes, &att.file_name) {
            Ok(frames) => {
                let video_model = effective_video_model(settings);
                match describe_video_with_model(
                    settings,
                    &video_model,
                    api_key,
                    &frames,
                    &att.file_name,
                    token_ctx,
                    cancel,
                )
                .await
                {
                    Ok(desc) => {
                        att.derived_text = Some(desc.clone());
                        return Ok((
                            None,
                            Some(with_saved_paths(
                                att,
                                format!("[Video: {}]\n{}", att.file_name, desc),
                            )),
                        ));
                    }
                    Err(e) => {
                        log::warn!(
                            "media: video understanding failed for {}: {:#}",
                            att.file_name,
                            e
                        );
                        return Ok((
                            None,
                            Some(crate::media::deps_hint::attachment_processing_failed_hint(
                                &att.file_name,
                                &format!("视频理解失败：{e}"),
                                att.storage_rel_path.as_deref(),
                            )),
                        ));
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "media: video frame extraction failed for {}: {:#}",
                    att.file_name,
                    e
                );
                return Ok((
                    None,
                    Some(crate::media::deps_hint::video_frame_extraction_failed(
                        &att.file_name,
                        &e.to_string(),
                        att.storage_rel_path.as_deref(),
                    )),
                ));
            }
        }
    }

    log::warn!(
        "media: unsupported attachment kind={} mime={} name={}",
        att.kind,
        mime,
        att.file_name
    );
    Ok((
        None,
        Some(crate::media::deps_hint::unsupported_attachment_hint(
            &att.file_name,
            &mime,
            att.storage_rel_path.as_deref(),
        )),
    ))
}

struct AttachmentProcessOutcome {
    file_name: String,
    injected: Option<String>,
    replace_existing: bool,
}

async fn reprocess_failed_attachments(
    history: &mut [ChatMessage],
    plan: MediaRetryPlan,
    settings: &ModelSettings,
    conversation_id: &str,
    api_key: &str,
    token_ctx: &MediaTokenContext,
    primary_vision: bool,
    cancel: &CancellationToken,
) -> Result<()> {
    let indices = message_indices_for_retry(history, plan);
    if indices.is_empty() {
        return Ok(());
    }
    log::info!(
        "media: retry {:?} on {} user message(s)",
        plan.scope,
        indices.len()
    );

    for msg_idx in indices {
        let msg_content = history[msg_idx].content.clone();
        let mut outcomes: Vec<AttachmentProcessOutcome> = Vec::new();
        let mut inline_images: Vec<String> = Vec::new();

        {
            let msg = &mut history[msg_idx];
            let Some(attachments) = msg.attachments.as_mut() else {
                continue;
            };
            for att in attachments.iter_mut() {
                if !should_retry_attachment(plan.scope, &msg_content, att) {
                    continue;
                }
                log::info!(
                    "media: reprocessing attachment {} from storage {:?}",
                    att.file_name,
                    att.storage_rel_path
                );
                match load_attachment_bytes(att) {
                    Ok(bytes) => {
                        match process_attachment_with_bytes(
                            settings,
                            conversation_id,
                            att,
                            &bytes,
                            primary_vision,
                            api_key,
                            token_ctx,
                            cancel,
                        )
                        .await
                        {
                            Ok((inline_b64, injected)) => {
                                if let Some(b64) = inline_b64 {
                                    inline_images.push(b64);
                                }
                                outcomes.push(AttachmentProcessOutcome {
                                    file_name: att.file_name.clone(),
                                    injected,
                                    replace_existing: content_has_media_block_for_file(
                                        &msg.content,
                                        &att.file_name,
                                    ),
                                });
                            }
                            Err(e) => {
                                log::warn!(
                                    "media: retry failed for {}: {:#}",
                                    att.file_name,
                                    e
                                );
                                outcomes.push(AttachmentProcessOutcome {
                                    file_name: att.file_name.clone(),
                                    injected: Some(
                                        crate::media::deps_hint::attachment_processing_failed_hint(
                                            &att.file_name,
                                            &e.to_string(),
                                            att.storage_rel_path.as_deref(),
                                        ),
                                    ),
                                    replace_existing: content_has_media_block_for_file(
                                        &msg.content,
                                        &att.file_name,
                                    ),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "media: retry load bytes failed for {}: {:#}",
                            att.file_name,
                            e
                        );
                    }
                }
            }
        }

        if !inline_images.is_empty() {
            let msg = &mut history[msg_idx];
            let existing = msg.images_base64.take().unwrap_or_default();
            let mut merged = existing;
            merged.extend(inline_images);
            msg.images_base64 = Some(merged);
        }

        for outcome in outcomes {
            let Some(block) = outcome.injected else {
                continue;
            };
            let msg = &mut history[msg_idx];
            if outcome.replace_existing {
                msg.content =
                    replace_media_injection(&msg.content, &outcome.file_name, Some(&block));
            } else if msg.content.trim().is_empty() {
                msg.content = block;
            } else {
                msg.content = format!("{}\n\n{}", msg.content, block);
            }
        }
    }
    Ok(())
}

/// Persist wire attachments, apply vision inline or imageModel understanding, inject document text.
pub async fn apply_media_to_history(
    history: &mut [ChatMessage],
    settings: &ModelSettings,
    run_id: &str,
    conversation_id: &str,
    api_key: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    let primary_vision = model_supports_vision(settings);
    let token_ctx = MediaTokenContext {
        run_id: run_id.to_string(),
        conversation_id: conversation_id.to_string(),
    };

    let mut retry_plans: Vec<MediaRetryPlan> = Vec::new();
    if let Some(plan) = detect_media_retry_plan(history) {
        retry_plans.push(plan);
    } else if let Some(plan) = auto_ffmpeg_video_retry_plan(history) {
        retry_plans.push(plan);
    }
    for plan in retry_plans {
        if let Err(e) = reprocess_failed_attachments(
            history,
            plan,
            settings,
            conversation_id,
            api_key,
            &token_ctx,
            primary_vision,
            cancel,
        )
        .await
        {
            log::warn!("media: reprocess_failed_attachments failed: {:#}", e);
        }
    }

    for msg in history.iter_mut() {
        if !matches!(msg.role, Role::User) {
            continue;
        }
        let Some(mut attachments) = msg.attachments.take() else {
            continue;
        };
        if attachments.is_empty() {
            continue;
        }

        let mut inline_images: Vec<String> = Vec::new();
        let mut injected_parts: Vec<String> = Vec::new();
        let mut content_replacements: Vec<(String, String)> = Vec::new();

        for att in attachments.iter_mut() {
            if att.content_base64.is_none() {
                if let Some(ref text) = att.derived_text {
                    let label = if att.kind == "audio" || is_audio_mime(&att.mime_type) {
                        "Audio"
                    } else if is_image_mime(&att.mime_type) || att.kind == "image" {
                        "Image"
                    } else {
                        "Attachment"
                    };
                    let block = with_saved_paths(
                        att,
                        format!("[{label}: {}]\n{}", att.file_name, text),
                    );
                    if content_has_media_block_for_file(&msg.content, &att.file_name) {
                        content_replacements.push((att.file_name.clone(), block));
                    } else {
                        injected_parts.push(block);
                    }
                }
                continue;
            }
            match process_attachment(
                settings,
                conversation_id,
                att,
                primary_vision,
                api_key,
                &token_ctx,
                cancel,
            )
            .await
            {
                Ok((inline_b64, injected)) => {
                    if let Some(b64) = inline_b64 {
                        inline_images.push(b64);
                    }
                    if let Some(part) = injected {
                        injected_parts.push(part);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "media: failed to process attachment {}: {:#}",
                        att.file_name,
                        e
                    );
                    injected_parts.push(
                        crate::media::deps_hint::attachment_processing_failed_hint(
                            &att.file_name,
                            &e.to_string(),
                            att.storage_rel_path.as_deref(),
                        ),
                    );
                }
            }
        }

        if !inline_images.is_empty() {
            let existing = msg.images_base64.take().unwrap_or_default();
            let mut merged = existing;
            merged.extend(inline_images);
            msg.images_base64 = Some(merged);
        }

        for (file_name, block) in content_replacements {
            msg.content = replace_media_injection(&msg.content, &file_name, Some(&block));
        }

        if !injected_parts.is_empty() {
            let block = injected_parts.join("\n\n");
            if msg.content.trim().is_empty() {
                msg.content = block;
            } else {
                msg.content = format!("{}\n\n{}", msg.content, block);
            }
        }

        msg.attachments = Some(attachments);
    }
    Ok(())
}
