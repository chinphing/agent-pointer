use anyhow::{Context, Result};
use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::ChatMediaPreview;
use crate::storage::{app_data_dir, sanitize_storage_dir_segment};
use crate::user_storage::session_user_id_for_conversation;

use super::access::{
    assert_app_media_preview_allowed, is_user_filesystem_path, path_has_traversal,
};
use super::filename::{allocate_unique_stored_basename, safe_attachment_basename};
use super::layout::{build_storage_rel, parse_storage_rel, verify_storage_rel_access};
use super::path_hint::MEDIA_URI_SCHEME;
use super::resolve::resolve_local_media_path;

pub const CONVERSATION_MEDIA_DIR: &str = "conversation-media";

pub fn conversation_media_root() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(CONVERSATION_MEDIA_DIR))
}

pub fn media_abs_path(storage_rel_path: &str) -> Result<PathBuf> {
    if let Err(e) = verify_storage_rel_access(storage_rel_path) {
        log::info!("media_abs_path verify_storage_rel_access FAILED: {e:#}");
        return Err(e);
    }
    media_abs_path_unscoped(storage_rel_path)
}

/// Resolve a storage rel path without session-user scoping (server outbound / allowlisted reads).
pub fn media_abs_path_unscoped(storage_rel_path: &str) -> Result<PathBuf> {
    let rel = storage_rel_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("invalid media rel path");
    }
    let primary = conversation_media_root()?.join(rel);
    if primary.is_file() {
        return Ok(primary);
    }
    // Legacy `{conv}/{file}` paths remain readable after user-scoped layout migration.
    if let Ok(parsed) = parse_storage_rel(rel) {
        if !parsed.legacy {
            let legacy = conversation_media_root()?
                .join(&parsed.conversation_segment)
                .join(&parsed.file_name);
            if legacy.is_file() {
                return Ok(legacy);
            }
        }
    }
    // `generated-media/…` and other app-data subtrees (not under conversation-media/).
    if let Ok(data_dir) = app_data_dir() {
        let under_data = data_dir.join(rel);
        if under_data.is_file() {
            return Ok(under_data);
        }
    }
    Ok(primary)
}

/// True when `path` resolves under the Pointer app data directory.
pub fn path_under_app_data(path: &Path) -> Result<bool> {
    let root = app_data_dir()?.canonicalize().context("app data dir")?;
    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve media path {}", path.display()))?;
    Ok(canonical.starts_with(&root))
}

/// Map an on-disk file under app data to a storage rel path for API / IM delivery.
pub fn app_data_media_rel_from_abs(path: &Path) -> Option<String> {
    let data_dir = app_data_dir().ok()?;
    let abs = path.canonicalize().ok()?;
    let root = data_dir.canonicalize().ok()?;
    let rel = abs.strip_prefix(&root).ok()?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = rel_str.strip_prefix("conversation-media/") {
        return Some(stripped.to_string());
    }
    if rel_str.starts_with("generated-media/") {
        return Some(rel_str);
    }
    None
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
    let session_user_id = session_user_id_for_conversation(conversation_id);
    let dir = conversation_media_root()?
        .join(crate::user_storage::user_storage_segment(&session_user_id))
        .join(&conv);
    fs::create_dir_all(&dir).context("媒体目录创建失败")?;
    let safe_name = safe_attachment_basename(file_name);
    let stored_name = allocate_unique_stored_basename(&dir, &safe_name, file_name)
        .ok_or_else(|| anyhow::anyhow!("unique attachment filename allocation failed"))?;
    let file_path = dir.join(&stored_name);
    fs::write(&file_path, bytes).context("write attachment file")?;
    let rel = conversation_media_abs_to_rel(&file_path).unwrap_or_else(|| {
        build_storage_rel(&session_user_id, conversation_id, &stored_name)
    });
    log::info!("save_attachment_bytes conv={conversation_id} id={id} -> {} rel={} ({} bytes)", file_path.display(), rel, bytes.len());
    Ok(rel)
}

pub fn read_media_bytes(storage_rel_path: &str) -> Result<Vec<u8>> {
    let path = match media_abs_path(storage_rel_path) {
        Ok(p) => p,
        Err(e) if e.to_string().contains("media access denied") => {
            log::info!(
                "read_media_bytes: session access denied for {storage_rel_path}, trying unscoped"
            );
            media_abs_path_unscoped(storage_rel_path)?
        }
        Err(e) => return Err(e),
    };
    fs::read(&path).with_context(|| format!("read media file {}", path.display()))
}

/// Absolute path, MIME, and file name for HTTP download/stream handlers (storage rel).
pub fn chat_media_file_meta(storage_rel_path: &str) -> Result<(PathBuf, String, String)> {
    let path = media_abs_path(storage_rel_path)?;
    media_file_meta_from_path(&path)
}

/// Absolute path, MIME, and file name for HTTP handlers resolving arbitrary media refs.
pub fn chat_media_ref_file_meta(media_ref: &str) -> Result<(PathBuf, String, String)> {
    let trimmed = media_ref.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media ref");
    }
    let rel = trimmed
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(trimmed);
    let path = resolve_local_media_path(rel)?;
    if !path.is_file() {
        anyhow::bail!("media file not found: {}", path.display());
    }
    media_file_meta_from_path(&path)
}

fn media_file_meta_from_path(path: &Path) -> Result<(PathBuf, String, String)> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime_type = mime_from_path(path);
    Ok((path.to_path_buf(), mime_type, file_name))
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

/// Parse storage rel into `(conversation_segment, attachment_id_stem)`.
/// Supports legacy `{conv}/{file}` and `{user}/{conv}/{file}` layouts.
pub fn parse_conversation_media_ids(storage_rel_path: &str) -> Option<(String, String)> {
    let parsed = parse_storage_rel(storage_rel_path).ok()?;
    let conv = parsed.conversation_segment;
    let stem = parsed
        .file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(parsed.file_name.as_str());
    if conv.is_empty() || stem.is_empty() {
        return None;
    }
    Some((conv, stem.to_string()))
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
