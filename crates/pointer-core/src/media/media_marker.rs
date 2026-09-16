//! `MEDIA:<path>[?attachmentId=<id>]` — wire format for chat history and model I/O.

use crate::models::MediaAttachment;
use crate::tools::file::{resolve_tool_workspace_root, resolve_within_workspace_root};
use std::path::{Path, PathBuf};

pub const ATTACHMENT_ID_QUERY_KEY: &str = "attachmentId";
const MEDIA_PREFIX: &str = "MEDIA:";

/// If `trimmed` begins with `MEDIA:` (ASCII, case-insensitive), return the suffix after it.
///
/// Uses [`str::get`] so Chinese / markdown table lines never panic on a byte slice
/// (e.g. `| 生成…` where byte 6 sits inside `成`).
fn media_marker_suffix(trimmed: &str) -> Option<&str> {
    let prefix = trimmed.get(..MEDIA_PREFIX.len())?;
    if prefix.eq_ignore_ascii_case(MEDIA_PREFIX) {
        Some(&trimmed[MEDIA_PREFIX.len()..])
    } else {
        None
    }
}

/// Split `path?attachmentId=…` (query only recognized for this key).
pub fn split_attachment_id_query(raw: &str) -> (String, Option<String>) {
    let s = raw.trim();
    let key = format!("?{ATTACHMENT_ID_QUERY_KEY}=");
    let Some(i) = s.rfind(&key) else {
        return (s.to_string(), None);
    };
    let path = s[..i].trim_end();
    if path.is_empty() {
        return (s.to_string(), None);
    }
    let id_raw = s[i + key.len()..].trim();
    let id = id_raw
        .split(|c: char| c.is_whitespace() || c == '&')
        .next()
        .unwrap_or("")
        .trim();
    if id.is_empty() {
        return (path.to_string(), None);
    }
    (path.to_string(), Some(id.to_string()))
}

pub fn format_media_marker(path: &str, attachment_id: &str) -> String {
    let path = path.trim();
    let id = attachment_id.trim();
    if id.is_empty() {
        format!("MEDIA:{path}")
    } else {
        format!("MEDIA:{path}?{ATTACHMENT_ID_QUERY_KEY}={id}")
    }
}

/// Prefer a WORKING_DIR-relative path when `abs` is under the workspace root.
pub fn media_marker_path_for_file(abs: &Path, workspace_root: Option<&Path>) -> String {
    if let Some(root) = workspace_root {
        if let Ok(rel) = abs.strip_prefix(root) {
            let s = rel.to_string_lossy().replace('\\', "/");
            if !s.is_empty() {
                return s;
            }
        }
    }
    abs.display().to_string()
}

pub fn workspace_root_for_media() -> Option<PathBuf> {
    resolve_tool_workspace_root().ok()
}

/// Resolve a MEDIA path (absolute, `pointer-media://`, or WORKING_DIR-relative).
pub fn resolve_media_marker_path(path: &str) -> Option<PathBuf> {
    let (path, _) = split_attachment_id_query(path);
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(p) = crate::media::media_ref::resolve_media_ref(trimmed) {
        if p.is_file() {
            return Some(p);
        }
    }
    let root = workspace_root_for_media()?;
    match resolve_within_workspace_root(&root, trimmed) {
        Ok(p) if p.is_file() => Some(p),
        _ => None,
    }
}

fn attachment_abs_path(att: &MediaAttachment) -> Option<PathBuf> {
    if let Some(local) = att
        .local_abs_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let p = PathBuf::from(local);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(rel) = att
        .storage_rel_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Ok(p) = crate::media::store::media_abs_path(rel) {
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Display path for a persisted attachment (relative to WORKING_DIR when possible).
pub fn media_marker_path_for_attachment(att: &MediaAttachment) -> Option<String> {
    let abs = attachment_abs_path(att)?;
    let root = workspace_root_for_media();
    Some(media_marker_path_for_file(&abs, root.as_deref()))
}

/// Build `MEDIA:…?attachmentId=` lines for user-upload attachments.
pub fn format_media_markers_for_attachments(attachments: &[MediaAttachment]) -> String {
    let mut lines = Vec::new();
    for att in attachments {
        let id = att.id.trim();
        if id.is_empty() {
            continue;
        }
        let Some(path) = media_marker_path_for_attachment(att) else {
            continue;
        };
        lines.push(format_media_marker(&path, id));
    }
    lines.join("\n")
}

/// Ensure user `content` includes MEDIA markers for each attachment (idempotent by id).
pub fn ensure_user_content_media_markers(content: &str, attachments: &[MediaAttachment]) -> String {
    if attachments.is_empty() {
        return content.to_string();
    }
    let existing_ids: std::collections::HashSet<String> = content
        .lines()
        .filter_map(|line| {
            let rest = media_marker_suffix(line.trim())?;
            split_attachment_id_query(rest.trim()).1
        })
        .collect();
    let mut extra = Vec::new();
    for att in attachments {
        let id = att.id.trim();
        if id.is_empty() || existing_ids.contains(id) {
            continue;
        }
        let Some(path) = media_marker_path_for_attachment(att) else {
            continue;
        };
        extra.push(format_media_marker(&path, id));
    }
    if extra.is_empty() {
        return content.to_string();
    }
    let base = content.trim_end();
    if base.is_empty() {
        extra.join("\n")
    } else {
        format!("{base}\n\n{}", extra.join("\n"))
    }
}

/// Rewrite `MEDIA:` lines to include `?attachmentId=` matched from registered attachments.
pub fn rewrite_media_markers_with_attachment_ids(
    content: &str,
    attachments: &[MediaAttachment],
) -> String {
    if attachments.is_empty() {
        return content.to_string();
    }
    let root = workspace_root_for_media();
    let mut by_id: std::collections::HashMap<&str, &MediaAttachment> =
        std::collections::HashMap::new();
    let mut by_abs: std::collections::HashMap<String, &MediaAttachment> =
        std::collections::HashMap::new();
    for att in attachments {
        let id = att.id.trim();
        if !id.is_empty() {
            by_id.insert(id, att);
        }
        if let Some(abs) = attachment_abs_path(att) {
            by_abs.insert(abs.to_string_lossy().to_string(), att);
            if let Ok(canon) = abs.canonicalize() {
                by_abs.insert(canon.to_string_lossy().to_string(), att);
            }
        }
    }

    let mut out_lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let Some(rest) = media_marker_suffix(trimmed) else {
            out_lines.push(line.to_string());
            continue;
        };
        let (path_part, id_opt) = split_attachment_id_query(rest.trim());
        let att = id_opt
            .as_deref()
            .and_then(|id| by_id.get(id).copied())
            .or_else(|| {
                let resolved = resolve_media_marker_path(&path_part)?;
                let key = resolved.to_string_lossy().to_string();
                by_abs.get(&key).copied().or_else(|| {
                    resolved
                        .canonicalize()
                        .ok()
                        .and_then(|c| by_abs.get(&c.to_string_lossy().to_string()).copied())
                })
            });
        let Some(att) = att else {
            out_lines.push(line.to_string());
            continue;
        };
        let path = media_marker_path_for_attachment(att)
            .unwrap_or_else(|| media_marker_path_for_file(Path::new(&path_part), root.as_deref()));
        out_lines.push(format_media_marker(&path, att.id.trim()));
    }
    out_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_query_roundtrip() {
        let (p, id) = split_attachment_id_query("/tmp/a.xlsx?attachmentId=9f2f0b9a8b7c");
        assert_eq!(p, "/tmp/a.xlsx");
        assert_eq!(id.as_deref(), Some("9f2f0b9a8b7c"));
        assert_eq!(
            format_media_marker(&p, id.as_deref().unwrap()),
            "MEDIA:/tmp/a.xlsx?attachmentId=9f2f0b9a8b7c"
        );
    }

    #[test]
    fn ensure_user_markers_appends() {
        let att = MediaAttachment {
            id: "abc123abc123".into(),
            kind: "document".into(),
            mime_type: "text/plain".into(),
            file_name: "a.txt".into(),
            size_bytes: 1,
            storage_rel_path: None,
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        // No resolvable file → no marker appended.
        let out = ensure_user_content_media_markers("hello", &[att]);
        assert_eq!(out, "hello");
    }

    /// Repro: Chinese markdown table (`| 生成…`) must not panic when rewriting ids.
    #[test]
    fn rewrite_chinese_table_lines_does_not_panic() {
        use std::io::Write;
        let dir =
            std::env::temp_dir().join(format!("pointer-media-marker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.xlsx");
        {
            let mut f = std::fs::File::create(&file).unwrap();
            f.write_all(b"x").unwrap();
        }
        let path = file.display().to_string();
        let att = MediaAttachment {
            id: "9f2f0b9a8b7c".into(),
            kind: "document".into(),
            mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            file_name: "out.xlsx".into(),
            size_bytes: 1,
            storage_rel_path: None,
            content_base64: None,
            derived_text: None,
            local_abs_path: Some(path.clone()),
            remote_url: None,
            oss_object_key: None,
        };
        let content = format!(
            "| 生成时间 | 交付时间 |\n| --- | --- |\n最终版就是下面这份 ——\n\nMEDIA:{path}"
        );
        let out = rewrite_media_markers_with_attachment_ids(&content, &[att]);
        assert!(out.contains("attachmentId=9f2f0b9a8b7c"));
        assert!(out.contains("| 生成时间 |"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
