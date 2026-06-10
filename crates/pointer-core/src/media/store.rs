use anyhow::{Context, Result};
use base64::Engine;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::ChatMediaPreview;
use crate::storage::app_data_dir;

use super::access::{
    assert_app_media_preview_allowed, is_user_filesystem_path, normalize_user_path,
    path_has_traversal,
};
use super::path_hint::MEDIA_URI_SCHEME;

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
    let conv = conversation_id.trim();
    let id = attachment_id.trim();
    if conv.is_empty() || id.is_empty() {
        anyhow::bail!("conversation_id and attachment_id required");
    }
    let dir = conversation_media_root()?.join(conv);
    fs::create_dir_all(&dir).context("create conversation-media dir")?;
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let file_path = dir.join(format!("{id}{ext}"));
    fs::write(&file_path, bytes).context("write attachment file")?;
    Ok(format!("{conv}/{id}{ext}"))
}

pub fn read_media_bytes(storage_rel_path: &str) -> Result<Vec<u8>> {
    let path = media_abs_path(storage_rel_path)?;
    fs::read(&path).with_context(|| format!("read media file {}", path.display()))
}

pub fn read_chat_media_preview(storage_rel_path: &str) -> Result<ChatMediaPreview> {
    let path = media_abs_path(storage_rel_path)?;
    read_file_preview(&path, extra_roots_empty())
}

/// Preview a media reference from assistant `MEDIA:` markers or attachment metadata.
pub fn read_media_ref_preview(media_ref: &str, extra_roots: &[String]) -> Result<ChatMediaPreview> {
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
        return read_file_preview(&path, extra_roots);
    }

    if path_has_traversal(rel) {
        anyhow::bail!("media path traversal not allowed: {rel}");
    }

    let path = resolve_filesystem_ref(rel)?;
    assert_app_media_preview_allowed(&path, extra_roots)?;
    read_file_preview(&path, extra_roots)
}

fn extra_roots_empty() -> &'static [String] {
    &[]
}

fn resolve_filesystem_ref(raw: &str) -> Result<PathBuf> {
    let path = normalize_user_path(raw)?;
    if path.is_absolute() && path.is_file() {
        return Ok(path);
    }
    if path.is_file() {
        return Ok(path);
    }

    if let Ok(data_dir) = app_data_dir() {
        let under_data = data_dir.join(raw.trim_start_matches('/'));
        if under_data.is_file() {
            return Ok(under_data);
        }
        let under_media = data_dir.join(CONVERSATION_MEDIA_DIR).join(raw.trim_start_matches('/'));
        if under_media.is_file() {
            return Ok(under_media);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let under_cwd = cwd.join(raw);
        if under_cwd.is_file() {
            return Ok(under_cwd);
        }
    }

    anyhow::bail!("media file not found: {raw}")
}

fn read_file_preview(path: &Path, extra_roots: &[String]) -> Result<ChatMediaPreview> {
    assert_app_media_preview_allowed(path, extra_roots)?;
    let bytes = fs::read(path).with_context(|| format!("read media file {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let mime_type = mime_from_path(path);
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
