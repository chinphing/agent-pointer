//! API-only user attachment manifest for LLM context (not persisted in `msg.content`).

use crate::models::MediaAttachment;
use serde_json::{json, Value};

use super::path_hint::MEDIA_URI_SCHEME;
use super::store::media_abs_path;

pub const USER_ATTACHMENTS_MARKER: &str = "<!-- pointer-user-attachments -->";
pub const DELIVERED_ATTACHMENTS_MARKER: &str = "<!-- pointer-delivered-attachments -->";
pub const ATTACHMENT_NEEDS_INTENT_MARKER: &str = "<!-- pointer-attachment-needs-intent -->";

pub fn attachment_ref_uri(storage_rel_path: &str) -> String {
    format!(
        "{MEDIA_URI_SCHEME}{}",
        storage_rel_path.trim().trim_start_matches('/')
    )
}

pub fn attachment_has_local(att: &MediaAttachment) -> bool {
    if att
        .storage_rel_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
    {
        return true;
    }
    att.local_abs_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
}

pub fn attachment_has_remote(att: &MediaAttachment) -> bool {
    att.remote_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
}

fn attachment_has_wire(att: &MediaAttachment) -> bool {
    att.content_base64
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
}

/// True when the attachment has no resolvable local path, remote URL, or wire bytes.
pub fn attachment_persist_failed(att: &MediaAttachment) -> bool {
    !attachment_has_local(att) && !attachment_has_remote(att) && !attachment_has_wire(att)
}

pub fn attachment_local_abs_path(att: &MediaAttachment) -> Option<String> {
    if let Some(rel) = att
        .storage_rel_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return media_abs_path(rel)
            .ok()
            .map(|p| p.display().to_string())
            .filter(|s| !s.is_empty());
    }
    att.local_abs_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn attachment_sandbox_path(att: &MediaAttachment) -> Option<String> {
    let rel = att
        .storage_rel_path
        .as_deref()?
        .trim()
        .trim_start_matches('/');
    let (_, sandbox_path) = rel.rsplit_once("/attachments/")?;
    let path = format!("attachments/{sandbox_path}");
    super::store::short_attachment_id_from_sandbox_rel(rel).map(|_| path)
}

/// JSON attachment summary for tools (`session_search`, etc.) — same fields as the API manifest.
pub fn attachment_summary_json(att: &MediaAttachment) -> Value {
    let mut obj = json!({
        "id": att.id,
        "attachmentId": att.id,
        "kind": att.kind,
        "fileName": att.file_name,
        "mimeType": att.mime_type,
        "sizeBytes": att.size_bytes,
    });
    if attachment_persist_failed(att) {
        obj["status"] = json!("failed");
        obj["error"] = json!("attachment not saved");
        return obj;
    }
    if let Some(rel) = att
        .storage_rel_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        obj["storageRelPath"] = json!(rel);
        obj["ref"] = json!(attachment_ref_uri(rel));
    }
    if let Some(path) = attachment_sandbox_path(att) {
        obj["sandboxPath"] = json!(path);
    }
    if let Some(path) = attachment_local_abs_path(att) {
        obj["localPath"] = json!(path);
    }
    if attachment_has_remote(att) {
        obj["remoteUrl"] = json!(att.remote_url.as_deref().unwrap_or("").trim());
    }
    if let Some(key) = att
        .oss_object_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        obj["ossObjectKey"] = json!(key);
    }
    if let Some(text) = att
        .derived_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        obj["derivedText"] = json!(text);
    }
    obj
}

pub fn attachment_summaries_json(atts: &[MediaAttachment]) -> Vec<Value> {
    atts.iter().map(attachment_summary_json).collect()
}

fn format_attachment_entry(index: usize, att: &MediaAttachment) -> String {
    let file_name = att.file_name.trim();
    let kind = att.kind.trim();
    let mime = att.mime_type.trim();
    let header = if mime.is_empty() {
        format!("{index}. **{file_name}** ({kind})")
    } else {
        format!("{index}. **{file_name}** ({kind}, {mime})")
    };

    let has_remote = attachment_has_remote(att);
    let has_local = attachment_has_local(att);

    if attachment_persist_failed(att) {
        return format!("{header}\n   - status: failed\n   - error: attachment not saved");
    }

    let size_line = if att.size_bytes > 0 {
        format!("   - sizeBytes: {}\n", att.size_bytes)
    } else {
        String::new()
    };

    let mut lines = vec![header, format!("   - attachmentId: {}", att.id)];
    if let Some(path) = attachment_sandbox_path(att) {
        lines.push(format!("   - sandboxPath: {path}"));
    }
    if has_remote && kind == "video" {
        let url = att.remote_url.as_deref().unwrap_or("").trim();
        lines.push(format!("{size_line}   - remoteUrl: {url}"));
    }
    if has_local {
        if let Some(local_path) = attachment_local_abs_path(att) {
            let rel = att.storage_rel_path.as_deref().unwrap_or("").trim();
            if rel.is_empty() {
                lines.push(format!("{size_line}   - localPath: {local_path}"));
            } else {
                let ref_uri = attachment_ref_uri(rel);
                lines.push(format!(
                    "{size_line}   - ref: {ref_uri}\n   - localPath: {local_path}"
                ));
            }
        } else if let Some(rel) = att
            .storage_rel_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let ref_uri = attachment_ref_uri(rel);
            lines.push(format!("{size_line}   - ref: {ref_uri}"));
        }
    } else if !has_remote || kind != "video" {
        lines.push(size_line.trim_end().to_string());
    }
    lines.join("\n")
}

/// Markdown block listing user attachments for the model (API request only).
pub fn format_user_attachments_api_manifest(attachments: &[MediaAttachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut lines = vec![USER_ATTACHMENTS_MARKER.to_string()];
    for (i, att) in attachments.iter().enumerate() {
        lines.push(format_attachment_entry(i + 1, att));
    }
    lines.join("\n")
}

/// Append attachment manifest to user text for the OpenAI API payload.
pub fn append_user_attachments_api_context(
    content: &str,
    attachments: &[MediaAttachment],
) -> String {
    let manifest = format_user_attachments_api_manifest(attachments);
    if manifest.is_empty() {
        return content.to_string();
    }
    let mut out = if content.trim().is_empty() {
        manifest
    } else {
        format!("{}\n\n{manifest}", content.trim_end())
    };
    if content.trim().is_empty() && !attachments.is_empty() {
        out.push_str("\n\n");
        out.push_str(ATTACHMENT_NEEDS_INTENT_MARKER);
    }
    out
}

/// Markdown block listing assistant-delivered attachments for the model (API request only).
pub fn format_delivered_attachments_api_manifest(attachments: &[MediaAttachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        DELIVERED_ATTACHMENTS_MARKER.to_string(),
        "Input metadata only — these files were already delivered. Do not echo this list; when the user asks to receive a file, deliver it with `MEDIA:<localPath>` or `MEDIA:<ref>` instead.".to_string(),
    ];
    for (i, att) in attachments.iter().enumerate() {
        lines.push(format_attachment_entry(i + 1, att));
    }
    lines.join("\n")
}

/// Append delivered-attachment manifest to assistant text for the OpenAI API payload.
///
/// Unlike [`append_user_attachments_api_context`], never adds
/// [`ATTACHMENT_NEEDS_INTENT_MARKER`] — these files were already sent to the user.
pub fn append_delivered_attachments_api_context(
    content: &str,
    attachments: &[MediaAttachment],
) -> String {
    let manifest = format_delivered_attachments_api_manifest(attachments);
    if manifest.is_empty() {
        return content.to_string();
    }
    if content.trim().is_empty() {
        manifest
    } else {
        format!("{}\n\n{manifest}", content.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaAttachment;

    #[test]
    fn manifest_includes_ref_and_filename() {
        let att = MediaAttachment {
            id: "id1".into(),
            kind: "image".into(),
            mime_type: "image/png".into(),
            file_name: "photo.png".into(),
            size_bytes: 100,
            storage_rel_path: Some("conv/a1.png".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        let m = format_user_attachments_api_manifest(&[att]);
        assert!(m.contains(USER_ATTACHMENTS_MARKER));
        assert!(m.contains("**photo.png**"));
        assert!(m.contains("pointer-media://conv/a1.png"));
        assert!(m.contains("localPath:"));
        assert!(m.contains("sizeBytes: 100"));
    }

    #[test]
    fn manifest_includes_video_remote_url() {
        let att = MediaAttachment {
            id: "v1".into(),
            kind: "video".into(),
            mime_type: "video/mp4".into(),
            file_name: "clip.mp4".into(),
            size_bytes: 1_000_000,
            storage_rel_path: Some("conv/v1".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: Some("https://bucket.oss-cn-hangzhou.aliyuncs.com/a/clip.mp4".into()),
            oss_object_key: Some("pointer-media-attachments/v1/clip.mp4".into()),
        };
        let m = format_user_attachments_api_manifest(&[att]);
        assert!(m.contains("remoteUrl: https://"));
    }

    #[test]
    fn needs_intent_when_only_attachments() {
        let att = MediaAttachment {
            id: "id1".into(),
            kind: "audio".into(),
            mime_type: "audio/wav".into(),
            file_name: "a.wav".into(),
            size_bytes: 1,
            storage_rel_path: Some("c/a.wav".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        let out = append_user_attachments_api_context("", &[att]);
        assert!(out.contains(ATTACHMENT_NEEDS_INTENT_MARKER));
    }

    #[test]
    fn summary_json_matches_manifest_fields() {
        let att = MediaAttachment {
            id: "id1".into(),
            kind: "image".into(),
            mime_type: "image/png".into(),
            file_name: "photo.png".into(),
            size_bytes: 100,
            storage_rel_path: Some("conv/a1.png".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        let j = attachment_summary_json(&att);
        assert_eq!(j["fileName"], "photo.png");
        assert!(j["ref"].as_str().unwrap().starts_with("pointer-media://"));
        assert!(j.get("localPath").is_some());
    }

    #[test]
    fn manifest_includes_sandbox_relative_path_for_new_attachment() {
        let att = MediaAttachment {
            id: "0a1b2c3d4e5f".into(),
            kind: "document".into(),
            mime_type: "application/pdf".into(),
            file_name: "report.pdf".into(),
            size_bytes: 1,
            storage_rel_path: Some(
                "session-sandboxes/user-1/attachments/0a1b2c3d4e5f_report.pdf".into(),
            ),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        let manifest = format_user_attachments_api_manifest(&[att]);
        assert!(manifest.contains("attachmentId: 0a1b2c3d4e5f"));
        assert!(manifest.contains("sandboxPath: attachments/0a1b2c3d4e5f_report.pdf"));
    }

    #[test]
    fn delivered_manifest_uses_distinct_marker_and_fields() {
        let att = MediaAttachment {
            id: "id1".into(),
            kind: "image".into(),
            mime_type: "image/png".into(),
            file_name: "out.png".into(),
            size_bytes: 50,
            storage_rel_path: Some("conv/out.png".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        let m = format_delivered_attachments_api_manifest(&[att]);
        assert!(m.contains(DELIVERED_ATTACHMENTS_MARKER));
        assert!(m.contains("Input metadata only"));
        assert!(m.contains("MEDIA:<localPath>"));
        assert!(!m.contains(USER_ATTACHMENTS_MARKER));
        assert!(!m.contains(ATTACHMENT_NEEDS_INTENT_MARKER));
        assert!(m.contains("**out.png**"));
        assert!(m.contains("pointer-media://conv/out.png"));
        assert!(m.contains("localPath:"));
    }

    #[test]
    fn append_delivered_empty_attachments_unchanged() {
        let out = append_delivered_attachments_api_context("caption", &[]);
        assert_eq!(out, "caption");
    }

    #[test]
    fn append_delivered_no_needs_intent_when_caption_empty() {
        let att = MediaAttachment {
            id: "id1".into(),
            kind: "file".into(),
            mime_type: "application/pdf".into(),
            file_name: "a.pdf".into(),
            size_bytes: 1,
            storage_rel_path: Some("c/a.pdf".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        let out = append_delivered_attachments_api_context("", &[att]);
        assert!(out.contains(DELIVERED_ATTACHMENTS_MARKER));
        assert!(!out.contains(ATTACHMENT_NEEDS_INTENT_MARKER));
    }
}
