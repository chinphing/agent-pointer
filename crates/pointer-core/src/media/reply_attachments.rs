//! Build persisted attachment metadata from assistant `MEDIA:` path references.

use std::path::Path;

use uuid::Uuid;

use crate::models::MediaAttachment;

use super::path_hint::MEDIA_URI_SCHEME;
use super::access::{is_user_filesystem_path, strip_file_uri};

pub fn attachments_from_reply_paths(paths: &[String]) -> Vec<MediaAttachment> {
    paths
        .iter()
        .filter_map(|raw| attachment_from_media_ref(raw))
        .collect()
}

fn attachment_from_media_ref(raw: &str) -> Option<MediaAttachment> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = strip_file_uri(trimmed).unwrap_or_else(|| trimmed.to_string());
    let rel = normalized
        .strip_prefix(MEDIA_URI_SCHEME)
        .map(str::trim)
        .unwrap_or(normalized.as_str());

    let (storage_rel_path, local_abs_path) = classify_media_ref(rel);
    let file_name = file_name_from_ref(rel, local_abs_path.as_deref());
    let kind = kind_from_file_name(&file_name);
    let mime_type = mime_from_file_name(&file_name);

    Some(MediaAttachment {
        id: format!("reply-media-{}", Uuid::new_v4()),
        kind,
        mime_type,
        file_name,
        size_bytes: 0,
        storage_rel_path,
        content_base64: None,
        derived_text: None,
        local_abs_path,
        remote_url: None,
        oss_object_key: None,
    })
}

fn classify_media_ref(rel: &str) -> (Option<String>, Option<String>) {
    if is_user_filesystem_path(rel) {
        return (None, Some(rel.to_string()));
    }
    if rel.contains('/') {
        return (Some(rel.to_string()), None);
    }
    (None, Some(rel.to_string()))
}

fn file_name_from_ref(rel: &str, local_abs_path: Option<&str>) -> String {
    if let Some(abs) = local_abs_path {
        return Path::new(abs)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
    }
    Path::new(rel)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string()
}

/// Infer attachment `kind` from a file name extension.
pub fn infer_attachment_kind(file_name: &str) -> String {
    kind_from_file_name(file_name)
}

/// Infer MIME type from file name, with optional caller hint.
pub fn infer_attachment_mime(file_name: &str, mime_hint: Option<&str>) -> String {
    if let Some(m) = mime_hint.map(str::trim).filter(|s| !s.is_empty()) {
        return m.to_string();
    }
    mime_from_file_name(file_name)
}

fn kind_from_file_name(file_name: &str) -> String {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => "image".into(),
        "mp4" | "webm" | "mov" | "mkv" => "video".into(),
        "mp3" | "wav" | "m4a" | "aac" | "ogg" | "flac" => "audio".into(),
        "pdf" | "txt" | "md" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "html"
        | "htm" | "json" | "csv" | "xml" | "yaml" | "yml" => "document".into(),
        "zip" | "tar" | "gz" | "7z" | "rar" => "file".into(),
        _ => "file".into(),
    }
}

fn mime_from_file_name(file_name: &str) -> String {
    match Path::new(file_name)
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
        Some("html") | Some("htm") => "text/html".into(),
        Some("json") => "application/json".into(),
        Some("csv") => "text/csv".into(),
        Some("txt") | Some("md") => "text/plain".into(),
        Some("mp4") => "video/mp4".into(),
        Some("mp3") => "audio/mpeg".into(),
        _ => "application/octet-stream".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_path_becomes_local_abs_path() {
        let atts = attachments_from_reply_paths(&["/Users/me/Desktop/baby.jpg".into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].local_abs_path.as_deref(), Some("/Users/me/Desktop/baby.jpg"));
        assert_eq!(atts[0].kind, "image");
    }

    #[test]
    fn pointer_media_becomes_storage_rel_path() {
        let atts = attachments_from_reply_paths(&["conv-id/att.png".into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].storage_rel_path.as_deref(), Some("conv-id/att.png"));
    }

    #[test]
    fn html_path_becomes_document_attachment() {
        let atts = attachments_from_reply_paths(&["/tmp/minesweeper.html".into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].kind, "document");
        assert_eq!(atts[0].mime_type, "text/html");
    }

    #[test]
    fn file_uri_normalizes_to_local_abs_path() {
        let atts =
            attachments_from_reply_paths(&["file:///C:/Users/me/game.html".into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(
            atts[0].local_abs_path.as_deref(),
            Some("C:/Users/me/game.html")
        );
    }

    #[test]
    fn tilde_path_becomes_local_abs_path() {
        let atts = attachments_from_reply_paths(&["~/Desktop/baby_cover.jpg".into()]);
        assert_eq!(atts.len(), 1);
        assert_eq!(
            atts[0].local_abs_path.as_deref(),
            Some("~/Desktop/baby_cover.jpg")
        );
        assert_eq!(atts[0].kind, "image");
    }
}
