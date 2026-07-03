//! Inbound attachment filename normalization (percent-decode, mojibake recovery, merge hints).

use std::path::Path;

use uuid::Uuid;

const STORED_ATTACHMENT_SUFFIX_LEN: usize = 8;
const STORED_ATTACHMENT_SUFFIX_ATTEMPTS: usize = 32;

/// Recovery path hint mode when media extraction failed or is unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPathMode {
    /// Binary / Office / media — use Skill or terminal, not file_read.
    Binary,
    /// Likely UTF-8 plain text — file_read may work as a fallback.
    TextLike,
}

pub fn normalize_inbound_filename(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .replace(['\r', '\n', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if trimmed.is_empty() {
        return String::new();
    }
    let basename = Path::new(&trimmed)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(trimmed.as_str());
    let decoded = percent_decode_if_needed(basename);
    fix_misdecoded_utf8(&decoded)
}

/// Pick the best display filename from IM message metadata and HTTP Content-Disposition.
pub fn merge_inbound_filename(
    message_hint: Option<String>,
    content_disposition_name: Option<String>,
) -> Option<String> {
    let hint = message_hint
        .as_deref()
        .map(normalize_inbound_filename)
        .filter(|s| !s.is_empty());
    let cd = content_disposition_name
        .as_deref()
        .map(normalize_inbound_filename)
        .filter(|s| !s.is_empty());

    match (hint.as_ref(), cd.as_ref()) {
        (None, None) => None,
        (Some(h), None) => Some(h.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(h), Some(c)) if looks_percent_encoded(h) && !looks_percent_encoded(c) => Some(c.clone()),
        (Some(h), Some(c)) if looks_percent_encoded(h) => Some(c.clone()),
        (Some(h), Some(_)) => Some(h.clone()),
    }
}

pub fn looks_percent_encoded(s: &str) -> bool {
    s.contains('%')
        && s.as_bytes().windows(3).any(|w| {
            w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit()
        })
}

fn percent_decode_if_needed(s: &str) -> String {
    if !looks_percent_encoded(s) {
        return s.to_string();
    }
    urlencoding::decode(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.to_string())
}

/// Reverse UTF-8 bytes misinterpreted as Latin-1/Windows-1252 (common in IM filenames).
fn fix_misdecoded_utf8(s: &str) -> String {
    if s.is_empty() || !s.bytes().any(|b| b >= 0x80) {
        return s.to_string();
    }
    if s.contains('\u{FFFD}') {
        return s.to_string();
    }
    let bytes: Vec<u8> = s.bytes().collect();
    match String::from_utf8(bytes) {
        Ok(fixed) if fixed != s && !fixed.contains('\u{FFFD}') => fixed,
        _ => s.to_string(),
    }
}

const TEXT_LIKE_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "log", "json", "yaml", "yml", "xml", "ini", "cfg",
    "conf", "env", "toml",
];

pub fn is_text_like_filename(file_name: &str) -> bool {
    extension_lower(file_name)
        .is_some_and(|ext| TEXT_LIKE_EXTENSIONS.contains(&ext.as_str()))
}

pub fn recovery_mode_for_filename(file_name: &str) -> RecoveryPathMode {
    if is_text_like_filename(file_name) {
        RecoveryPathMode::TextLike
    } else {
        RecoveryPathMode::Binary
    }
}

fn extension_lower(file_name: &str) -> Option<String> {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// Safe attachment basename for disk paths and OSS object names (preserves Unicode).
pub fn safe_attachment_basename(file_name: &str) -> String {
    let base = normalize_inbound_filename(file_name);
    if base.is_empty() {
        return String::new();
    }
    let sanitized: String = base
        .chars()
        .filter(|&ch| !ch.is_control() && !matches!(ch, '/' | '\\'))
        .map(|ch| if ch == ' ' { '_' } else { ch })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn extension_with_dot(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .map(|e| format!(".{e}"))
        .unwrap_or_default()
}

fn split_stored_name_parts(name: &str) -> (String, String) {
    let path = Path::new(name);
    let ext = extension_with_dot(name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .to_string();
    (stem, ext)
}

/// 8-char hex suffix for on-disk attachment names.
pub fn short_attachment_suffix() -> String {
    let hex = Uuid::new_v4().simple().to_string();
    hex[..STORED_ATTACHMENT_SUFFIX_LEN].to_string()
}

/// On-disk basename: `{stem}_{suffix}{ext}`; empty safe name → `attachment_{suffix}{ext}`.
pub fn stored_attachment_basename(safe_name: &str, suffix: &str, ext_fallback: &str) -> String {
    if safe_name.is_empty() {
        let ext = extension_with_dot(ext_fallback);
        return format!("attachment_{suffix}{ext}");
    }
    let (stem, ext) = split_stored_name_parts(safe_name);
    format!("{stem}_{suffix}{ext}")
}

/// Pick a stored basename that does not already exist in `dir` (regenerate suffix on collision).
pub fn allocate_unique_stored_basename(
    dir: &Path,
    safe_name: &str,
    ext_fallback: &str,
) -> Option<String> {
    for _ in 0..STORED_ATTACHMENT_SUFFIX_ATTEMPTS {
        let suffix = short_attachment_suffix();
        let candidate = stored_attachment_basename(safe_name, &suffix, ext_fallback);
        if !dir.join(&candidate).exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn looks_like_utf8_text_content(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let mut printable = 0usize;
    let mut control = 0usize;
    for ch in text.chars().take(8192) {
        let code = ch as u32;
        if ch == '\n' || ch == '\r' || ch == '\t' || ch == ' ' {
            printable += 1;
            continue;
        }
        if code < 32 || (code >= 0x7f && code <= 0x9f) {
            control += 1;
            continue;
        }
        printable += 1;
    }
    let total = printable + control;
    if total == 0 {
        return false;
    }
    printable as f64 / total as f64 > 0.85
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_percent_encoded_chinese_filename() {
        let raw = "(%E5%AE%8C%E6%95%B4)%E5%9B%9B%E5%B9%B4%E7%BA%A7%E5%A5%A5%E6%95%B0.doc";
        let out = normalize_inbound_filename(raw);
        assert!(out.contains('完'));
        assert!(out.ends_with(".doc"));
        assert!(!out.contains('%'));
    }

    #[test]
    fn merge_prefers_decoded_content_disposition_over_percent_hint() {
        let hint = Some("(%E5%AE%8C%E6%95%B4)report.doc".into());
        let cd = Some("(完整)report.doc".into());
        let merged = merge_inbound_filename(hint, cd).unwrap();
        assert!(merged.contains('完'));
        assert!(!merged.contains('%'));
    }

    #[test]
    fn safe_attachment_basename_preserves_chinese() {
        assert_eq!(
            safe_attachment_basename("像素蛋糕完整示例-0513.mp4"),
            "像素蛋糕完整示例-0513.mp4"
        );
    }

    #[test]
    fn safe_attachment_basename_strips_path_and_spaces() {
        assert_eq!(
            safe_attachment_basename("../../evil clip.mp4"),
            "evil_clip.mp4"
        );
    }

    #[test]
    fn stored_attachment_basename_puts_suffix_before_extension() {
        assert_eq!(
            stored_attachment_basename("report.pdf", "a1b2c3d4", ""),
            "report_a1b2c3d4.pdf"
        );
        assert_eq!(
            stored_attachment_basename("像素蛋糕完整示例-0513.mp4", "abcd1234", ""),
            "像素蛋糕完整示例-0513_abcd1234.mp4"
        );
    }

    #[test]
    fn stored_attachment_basename_empty_safe_name_uses_attachment_prefix() {
        assert_eq!(
            stored_attachment_basename("", "a1b2c3d4", "data.bin"),
            "attachment_a1b2c3d4.bin"
        );
    }

    #[test]
    fn short_attachment_suffix_is_eight_chars() {
        assert_eq!(short_attachment_suffix().len(), STORED_ATTACHMENT_SUFFIX_LEN);
    }

    #[test]
    fn text_like_extensions() {
        assert!(is_text_like_filename("notes.md"));
        assert!(is_text_like_filename("data.csv"));
        assert!(!is_text_like_filename("paper.doc"));
    }

    #[test]
    fn utf8_text_content_heuristic() {
        assert!(looks_like_utf8_text_content("四年级奥数练习题\n第二题"));
        assert!(!looks_like_utf8_text_content("\x00\x01\x02"));
    }
}
