use anyhow::{Context, Result};
use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::ChatMediaPreview;
use crate::storage::{app_data_dir, sanitize_storage_dir_segment};

use super::access::{
    assert_app_media_preview_allowed, is_user_filesystem_path, path_has_traversal,
};
use super::filename::safe_attachment_basename;
use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;

pub const CONVERSATION_MEDIA_DIR: &str = "conversation-media";

pub fn conversation_media_root() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(CONVERSATION_MEDIA_DIR))
}

pub fn media_abs_path(storage_rel_path: &str) -> Result<PathBuf> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("invalid media rel path");
    }
    Ok(conversation_media_root()?.join(rel))
}

pub fn save_attachment_bytes(
    conversation_id: &str,
    attachment_id: &str,
    bytes: &[u8],
    file_name: &str,
) -> Result<String> {
    let conv = sanitize_storage_dir_segment(conversation_id.trim());
    let id = attachment_id.trim();
    if conv.is_empty() || id.is_empty() {
        anyhow::bail!("conversation_id and attachment_id required");
    }
    let dir = conversation_media_root()?.join(&conv);
    fs::create_dir_all(&dir).context("媒体目录创建失败")?;
    let safe_name = safe_attachment_basename(file_name);
    let file_path = if safe_name.is_empty() {
        let ext = Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        dir.join(format!("{id}{ext}"))
    } else {
        dir.join(format!("{id}_{safe_name}"))
    };
    fs::write(&file_path, bytes).context("write attachment file")?;
    let rel = conversation_media_abs_to_rel(&file_path).unwrap_or_else(|| {
        if safe_name.is_empty() {
            let ext = Path::new(file_name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{e}"))
                .unwrap_or_default();
            format!("{conv}/{id}{ext}")
        } else {
            format!("{conv}/{id}_{safe_name}")
        }
    });
    Ok(rel)
}

pub fn read_media_bytes(storage_rel_path: &str) -> Result<Vec<u8>> {
    let path = media_abs_path(storage_rel_path)?;
    fs::read(&path).with_context(|| format!("read media file {}", path.display()))
}

/// `{conv}/{id}.bin` -> `{conv}/{id}.wav` when the wav file already exists on disk.
pub fn stored_wav_sibling_rel(storage_rel_path: &str) -> Option<String> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    let lower = rel.to_ascii_lowercase();
    if !lower.ends_with(".bin") {
        return None;
    }
    let wav_rel = format!("{}wav", &rel[..rel.len() - 3]);
    let path = match media_abs_path(&wav_rel) {
        Ok(p) => p,
        Err(_) => return None,
    };
    if path.is_file() {
        Some(wav_rel)
    } else {
        None
    }
}

/// Parse `conversation-media/{conv}/{attachment_id}.{ext}` into `(conv, attachment_id)`.
pub fn parse_conversation_media_ids(storage_rel_path: &str) -> Option<(String, String)> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    let (conv, file) = rel.split_once('/')?;
    if conv.is_empty() {
        return None;
    }
    let stem = file
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file);
    if stem.is_empty() {
        return None;
    }
    Some((conv.to_string(), stem.to_string()))
}

pub fn conversation_media_abs_to_rel(path: &Path) -> Option<String> {
    let root = match conversation_media_root() {
        Ok(r) => r,
        Err(_) => return None,
    };
    let rel = match path.strip_prefix(&root) {
        Ok(r) => r,
        Err(_) => return None,
    };
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn wav_sibling_abs_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".bin") {
        return None;
    }
    let wav_name = format!("{}wav", &name[..name.len() - 3]);
    let wav_path = path.with_file_name(wav_name);
    if wav_path.is_file() {
        Some(wav_path)
    } else {
        None
    }
}

pub fn read_chat_media_preview(storage_rel_path: &str) -> Result<ChatMediaPreview> {
    let path = media_abs_path(storage_rel_path)?;
    read_file_preview(&path)
}

/// Preview a media reference from assistant `MEDIA:` markers or attachment metadata.
pub fn read_media_ref_preview(media_ref: &str) -> Result<ChatMediaPreview> {
    let trimmed = media_ref.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media ref");
    }

    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);

    if !is_user_filesystem_path(rel) && rel.contains('/') {
        let path = media_abs_path(rel)?;
        return read_file_preview(&path);
    }

    if path_has_traversal(rel) {
        anyhow::bail!("media path traversal not allowed: {rel}");
    }

    let path = resolve_local_media_path(rel)?;
    assert_app_media_preview_allowed(&path)?;
    read_file_preview(&path)
}

fn read_file_preview(path: &Path) -> Result<ChatMediaPreview> {
    assert_app_media_preview_allowed(path)?;
    if let Some(wav_path) = wav_sibling_abs_path(path) {
        let bytes = fs::read(&wav_path)
            .with_context(|| format!("read persisted wav {}", wav_path.display()))?;
        let file_name = wav_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();
        return Ok(ChatMediaPreview {
            data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            mime_type: "audio/wav".into(),
            file_name,
        });
    }
    let bytes = fs::read(path).with_context(|| format!("read media file {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime_type = mime_from_path(path);
    let mime_lower = mime_type.to_ascii_lowercase();
    let needs_audio_transcode =
        mime_lower.starts_with("audio/") && mime_lower != "audio/wav"
            || file_name.to_ascii_lowercase().ends_with(".bin");
    if needs_audio_transcode && crate::media::ffmpeg::ffmpeg_available() {
        let storage_ctx = conversation_media_abs_to_rel(path).map(|rel| {
            let (conv, id) = parse_conversation_media_ids(&rel).unwrap_or((String::new(), String::new()));
            crate::media::audio::AudioStorageContext {
                storage_rel_path: Some(rel),
                conversation_id: conv,
                attachment_id: id,
            }
        });
        match crate::media::audio::prepare_audio_bytes_for_asr_cached(
            &bytes,
            &mime_type,
            &file_name,
            storage_ctx.as_ref(),
        ) {
            Ok(prepared) if prepared.mime_type == "audio/wav" => {
                return Ok(ChatMediaPreview {
                    data_base64: base64::engine::general_purpose::STANDARD.encode(&prepared.bytes),
                    mime_type: prepared.mime_type,
                    file_name: prepared.file_name,
                });
            }
            Err(e) => {
                log::warn!(
                    "media preview: audio transcode failed for {}: {:#}",
                    file_name,
                    e
                );
            }
            _ => {}
        }
    }
    Ok(ChatMediaPreview {
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        mime_type,
        file_name,
    })
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
        Some("txt") => "text/plain".into(),
        Some("md") => "text/markdown".into(),
        Some("json") => "application/json".into(),
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
