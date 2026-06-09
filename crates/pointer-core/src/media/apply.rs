use crate::media::capabilities::model_supports_vision;
use crate::media::store::save_attachment_bytes;
use crate::media::understand::{
    describe_image_with_model, describe_video_with_model, transcribe_audio_with_model,
};
use crate::models::{AgentModelRef, ChatMessage, MediaAttachment, ModelSettings, Role};
use anyhow::{Context, Result};
use base64::Engine;
use tokio_util::sync::CancellationToken;

pub const INLINE_IMAGE_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const HARD_IMAGE_MAX_BYTES: usize = 6 * 1024 * 1024;
const MAX_DOCUMENT_TEXT_BYTES: usize = 256 * 1024;

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

fn is_video_mime(mime: &str) -> bool {
    mime.trim().to_ascii_lowercase().starts_with("video/")
}

fn effective_audio_model(settings: &ModelSettings) -> AgentModelRef {
    if let Some(ref m) = settings.media_model_overrides.audio {
        if !m.model.trim().is_empty() {
            return m.clone();
        }
    }
    effective_image_model(settings)
}

fn effective_video_model(settings: &ModelSettings) -> AgentModelRef {
    if let Some(ref m) = settings.media_model_overrides.video {
        if !m.model.trim().is_empty() {
            return m.clone();
        }
    }
    effective_image_model(settings)
}

fn effective_image_model(settings: &ModelSettings) -> AgentModelRef {
    if let Some(ref m) = settings.media_model_overrides.image {
        if !m.model.trim().is_empty() {
            return m.clone();
        }
    }
    AgentModelRef {
        provider_id: "qwen".into(),
        model: "qwen3.5-plus".into(),
    }
}

fn decode_attachment_b64(att: &MediaAttachment) -> Result<Vec<u8>> {
    let raw = att
        .content_base64
        .as_deref()
        .context("attachment missing contentBase64")?
        .trim();
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .context("decode attachment base64")
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

async fn process_attachment(
    settings: &ModelSettings,
    conversation_id: &str,
    att: &mut MediaAttachment,
    primary_vision: bool,
    api_key: &str,
    cancel: &CancellationToken,
) -> Result<(Option<String>, Option<String>)> {
    let bytes = decode_attachment_b64(att)?;
    if att.storage_rel_path.as_deref().unwrap_or("").is_empty() {
        let rel = save_attachment_bytes(conversation_id, &att.id, &bytes, &att.file_name)?;
        att.storage_rel_path = Some(rel);
    }
    att.content_base64 = None;
    att.size_bytes = bytes.len() as u64;

    let mime = att.mime_type.clone();
    if is_image_mime(&mime) {
        if bytes.len() > HARD_IMAGE_MAX_BYTES {
            anyhow::bail!(
                "image {} exceeds {} MB limit",
                att.file_name,
                HARD_IMAGE_MAX_BYTES / (1024 * 1024)
            );
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
        let desc = describe_image_with_model(
            settings,
            &image_model,
            api_key,
            &b64,
            &mime,
            cancel,
        )
        .await?;
        att.derived_text = Some(desc.clone());
        return Ok((
            None,
            Some(format!("[Image: {}]\n{}", att.file_name, desc)),
        ));
    }

    if is_audio_mime(&mime) || att.kind == "audio" {
        if let Some(existing) = att.derived_text.as_ref().filter(|t| !t.trim().is_empty()) {
            att.content_base64 = None;
            return Ok((
                None,
                Some(format!("[Audio: {}]\n{}", att.file_name, existing)),
            ));
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let audio_model = effective_audio_model(settings);
        log::info!(
            "media: transcribing audio {} via model {}",
            att.file_name,
            audio_model.model
        );
        match transcribe_audio_with_model(
            settings,
            &audio_model,
            api_key,
            &b64,
            &mime,
            &att.file_name,
            cancel,
        )
        .await
        {
            Ok(transcript) => {
                att.derived_text = Some(transcript.clone());
                return Ok((
                    None,
                    Some(format!("[Audio: {}]\n{}", att.file_name, transcript)),
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
                    Some(format!(
                        "[Audio: {}] (transcription failed: {})",
                        att.file_name, e
                    )),
                ));
            }
        }
    }

    if is_text_document_mime(&mime) || att.kind == "document" {
        if mime == "application/pdf" || att.file_name.to_ascii_lowercase().ends_with(".pdf") {
            match crate::media::pdf::extract_pdf_text(&bytes, &att.file_name) {
                Ok(text) => {
                    att.derived_text = Some(text.clone());
                    return Ok((
                        None,
                        Some(format!("[PDF: {}]\n```\n{}\n```", att.file_name, text)),
                    ));
                }
                Err(e) => {
                    log::warn!(
                        "media: pdf extract failed for {}: {:#}",
                        att.file_name,
                        e
                    );
                    return Ok((
                        None,
                        Some(format!(
                            "[PDF: {}] (text extraction failed: {})",
                            att.file_name, e
                        )),
                    ));
                }
            }
        }
        let text = extract_document_text(&bytes, &att.file_name)?;
        att.derived_text = Some(text.clone());
        return Ok((
            None,
            Some(format!("[File: {}]\n```\n{}\n```", att.file_name, text)),
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
                Some(crate::media::deps_hint::video_ffmpeg_missing(&att.file_name)),
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
                    cancel,
                )
                .await
                {
                    Ok(desc) => {
                        att.derived_text = Some(desc.clone());
                        return Ok((
                            None,
                            Some(format!("[Video: {}]\n{}", att.file_name, desc)),
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
                            Some(format!(
                                "[Video: {}] (understanding failed: {})",
                                att.file_name, e
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
                    Some(format!(
                        "[Video: {}] (frame extraction failed: {})",
                        att.file_name,
                        e
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
        Some(format!(
            "[Attachment: {}] (unsupported type {}; not sent to model)",
            att.file_name, mime
        )),
    ))
}

/// Persist wire attachments, apply vision inline or imageModel understanding, inject document text.
pub async fn apply_media_to_history(
    history: &mut [ChatMessage],
    settings: &ModelSettings,
    conversation_id: &str,
    api_key: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    let primary_vision = model_supports_vision(settings);
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
                    injected_parts.push(format!("[{label}: {}]\n{}", att.file_name, text));
                }
                continue;
            }
            match process_attachment(settings, conversation_id, att, primary_vision, api_key, cancel)
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
                    injected_parts.push(format!(
                        "[Attachment: {}] (processing failed: {})",
                        att.file_name, e
                    ));
                }
            }
        }

        if !inline_images.is_empty() {
            let existing = msg.images_base64.take().unwrap_or_default();
            let mut merged = existing;
            merged.extend(inline_images);
            msg.images_base64 = Some(merged);
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
