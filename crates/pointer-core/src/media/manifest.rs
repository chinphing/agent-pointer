//! API-only user attachment manifest for LLM context (not persisted in `msg.content`).

use crate::models::MediaAttachment;

use super::path_hint::MEDIA_URI_SCHEME;
use super::store::media_abs_path;

pub const USER_ATTACHMENTS_MARKER: &str = "<!-- pointer-user-attachments -->";
pub const ATTACHMENT_NEEDS_INTENT_MARKER: &str = "<!-- pointer-attachment-needs-intent -->";

fn attachment_ref_uri(storage_rel_path: &str) -> String {
    format!("{MEDIA_URI_SCHEME}{}", storage_rel_path.trim().trim_start_matches('/'))
}

fn format_attachment_entry(index: usize, att: &MediaAttachment, persist_failed: bool) -> String {
    let file_name = att.file_name.trim();
    let kind = att.kind.trim();
    let mime = att.mime_type.trim();
    let header = if mime.is_empty() {
        format!("{index}. **{file_name}** ({kind})")
    } else {
        format!("{index}. **{file_name}** ({kind}, {mime})")
    };

    if persist_failed
        || att
            .storage_rel_path
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return format!("{header}\n   - status: failed\n   - error: attachment not saved");
    }

    let rel = att.storage_rel_path.as_deref().unwrap_or("").trim();
    let ref_uri = attachment_ref_uri(rel);
    let local_path = media_abs_path(rel)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| {
            log::warn!("media manifest: resolve local path for {rel}: {e:#}");
            String::new()
        });

    if local_path.is_empty() {
        format!("{header}\n   - ref: {ref_uri}")
    } else {
        format!(
            "{header}\n   - ref: {ref_uri}\n   - localPath: {local_path}"
        )
    }
}

/// Markdown block listing user attachments for the model (API request only).
pub fn format_user_attachments_api_manifest(attachments: &[MediaAttachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut lines = vec![USER_ATTACHMENTS_MARKER.to_string()];
    for (i, att) in attachments.iter().enumerate() {
        let failed = att
            .storage_rel_path
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
            && att
                .content_base64
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty();
        lines.push(format_attachment_entry(i + 1, att, failed));
    }
    lines.join("\n")
}

/// Append attachment manifest to user text for the OpenAI API payload.
pub fn append_user_attachments_api_context(content: &str, attachments: &[MediaAttachment]) -> String {
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
        };
        let m = format_user_attachments_api_manifest(&[att]);
        assert!(m.contains(USER_ATTACHMENTS_MARKER));
        assert!(m.contains("**photo.png**"));
        assert!(m.contains("pointer-media://conv/a1.png"));
        assert!(m.contains("localPath:"));
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
        };
        let out = append_user_attachments_api_context("", &[att]);
        assert!(out.contains(ATTACHMENT_NEEDS_INTENT_MARKER));
    }
}
