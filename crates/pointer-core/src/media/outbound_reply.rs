//! Parse outbound media markers from agent replies (OpenClaw `MEDIA:` convention).

use super::media_ref::resolve_media_ref;
use super::path_hint::MEDIA_URI_SCHEME;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const MEDIA_PREFIX: &str = "MEDIA:";

fn media_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)MEDIA:").expect("media prefix re"))
}

fn trim_trailing_path_punct(s: &str) -> &str {
    s.trim_end_matches(|c: char| matches!(c, ',' | ';' | ')' | ']' | '}' | '.' | '。' | '、'))
        .trim()
}

/// If `trimmed` begins with `MEDIA:` (ASCII, case-insensitive), return the suffix after it.
///
/// Uses [`str::get`] so non-ASCII line starts (e.g. Chinese bullets) never panic on a byte slice.
fn media_line_suffix(trimmed: &str) -> Option<&str> {
    let prefix = trimmed.get(..MEDIA_PREFIX.len())?;
    if prefix.eq_ignore_ascii_case(MEDIA_PREFIX) {
        Some(&trimmed[MEDIA_PREFIX.len()..])
    } else {
        None
    }
}

/// Path + how many bytes of `after_marker` (not including a leading match of `MEDIA:`)
/// were consumed to extract it.
struct ParsedMediaPath<'a> {
    path: &'a str,
    consumed: usize,
}

/// Parse the path that follows a `MEDIA:` marker.
///
/// Supports spaced absolute paths (e.g. macOS `…/Application Support/…`) and
/// optional `` ` `` / `"` / `'` quoting.
///
/// Unquoted: prefer the remainder of the line when that file exists; otherwise
/// fall back to the first whitespace-delimited token so
/// `Hello MEDIA:/tmp/a.png world` still works.
fn parse_media_path_after_marker(after_marker: &str) -> Option<ParsedMediaPath<'_>> {
    let trim_leading = after_marker.len() - after_marker.trim_start().len();
    let rest = after_marker.trim_start();
    if rest.is_empty() {
        return None;
    }
    let bytes = rest.as_bytes();
    let quote = bytes[0];
    if matches!(quote, b'`' | b'"' | b'\'') {
        let q = quote as char;
        if let Some(end) = rest[1..].find(q) {
            let inner = rest[1..1 + end].trim();
            if !inner.is_empty() {
                return Some(ParsedMediaPath {
                    path: inner,
                    consumed: trim_leading + 1 + end + 1,
                });
            }
        }
        // Unclosed quote: parse the remainder after the opening quote as unquoted.
        return parse_unquoted_media_path(&rest[1..], trim_leading + 1);
    }

    parse_unquoted_media_path(rest, trim_leading)
}

fn parse_unquoted_media_path(rest: &str, leading_skip: usize) -> Option<ParsedMediaPath<'_>> {
    let full = trim_trailing_path_punct(rest);
    if full.is_empty() {
        return None;
    }
    if reply_media_path_resolves(full) {
        return Some(ParsedMediaPath {
            path: full,
            consumed: leading_skip + full.len(),
        });
    }

    // Mid-sentence form: `Hello MEDIA:/tmp/a.png world` — try the first token.
    if let Some(token) = rest
        .split_whitespace()
        .next()
        .map(trim_trailing_path_punct)
        .filter(|t| !t.is_empty() && *t != full)
    {
        if reply_media_path_resolves(token) {
            return Some(ParsedMediaPath {
                path: token,
                consumed: leading_skip + token.len(),
            });
        }
    }

    // Neither candidate exists — keep the full remainder so spaced paths stay intact in UI.
    Some(ParsedMediaPath {
        path: full,
        consumed: leading_skip + full.len(),
    })
}

/// True when a reply media path / URI resolves to an existing file on disk.
///
/// Accepts absolute paths, `file://`, and `pointer-media://` (scheme stripped like
/// [`resolve_media_ref`]).
pub fn reply_media_path_resolves(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    resolve_media_ref(trimmed)
        .ok()
        .is_some_and(|p| p.is_file())
}

fn strip_resolved_inline_media(line: &str, media_paths: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(m) = media_prefix_re().find(rest) {
        out.push_str(&rest[..m.start()]);
        let after = &rest[m.end()..];
        let Some(parsed) = parse_media_path_after_marker(after) else {
            out.push_str(m.as_str());
            rest = after;
            continue;
        };
        let consumed = parsed.consumed.min(after.len());
        if reply_media_path_resolves(parsed.path) {
            media_paths.push(parsed.path.to_string());
        } else {
            // Keep the original marker + path text for failed delivery.
            out.push_str(&rest[m.start()..m.end() + consumed]);
        }
        rest = &after[consumed..];
    }
    out.push_str(rest);
    out
}

/// Split agent reply into user-visible text and resolvable local media path references.
///
/// `MEDIA:` markers are removed from visible text **only** when the referenced file exists.
/// Unresolvable paths stay in the visible body so delivery failures remain visible.
pub fn split_reply_media(reply: &str) -> (String, Vec<String>) {
    let mut media_paths: Vec<String> = Vec::new();
    let mut text_lines: Vec<String> = Vec::new();

    for line in reply.lines() {
        let trimmed = line.trim();
        if let Some(rest) = media_line_suffix(trimmed) {
            let Some(parsed) = parse_media_path_after_marker(rest) else {
                text_lines.push(line.to_string());
                continue;
            };
            if reply_media_path_resolves(parsed.path) {
                media_paths.push(parsed.path.to_string());
            } else {
                text_lines.push(line.to_string());
            }
            continue;
        }
        // Bare `pointer-media://…` line: treat as outbound when the file resolves
        // (same as `MEDIA:pointer-media://…`). Unresolved URIs stay visible.
        if trimmed.starts_with(MEDIA_URI_SCHEME) {
            if reply_media_path_resolves(trimmed) {
                media_paths.push(trimmed.to_string());
            } else {
                text_lines.push(line.to_string());
            }
            continue;
        }

        let line_text = strip_resolved_inline_media(line, &mut media_paths);
        if !line_text.trim().is_empty() {
            text_lines.push(line_text);
        }
    }

    let text = text_lines.join("\n");
    (text, media_paths)
}

/// Remove resolved `MEDIA:` and resolvable bare `pointer-media://` lines for App UI display.
pub fn strip_outbound_media_markers(text: &str) -> String {
    split_reply_media(text).0.trim().to_string()
}

/// Assistant raw output slice that may contain `MEDIA:` (plain text or response-tool JSON).
pub fn reply_media_source(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Value::Object(ref obj) = v {
            if obj.get("tool_name").and_then(|x| x.as_str()) == Some("response") {
                return obj
                    .get("tool_args")
                    .and_then(|a| a.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
    }
    raw.to_string()
}

/// Rebuild IM outbound reply text: user-visible body plus resolved `MEDIA:` lines from raw output.
pub fn im_outbound_reply_source(raw_content: Option<&str>, visible_content: Option<&str>) -> String {
    let Some(raw) = raw_content.map(str::trim).filter(|s| !s.is_empty()) else {
        return visible_content.unwrap_or("").trim().to_string();
    };

    let source = reply_media_source(raw);
    let (_, resolved_paths) = split_reply_media(&source);
    if resolved_paths.is_empty() {
        return visible_content.unwrap_or(raw).trim().to_string();
    }

    let visible = visible_content
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| split_reply_media(&source).0.trim().to_string());

    let mut out = visible;
    for path in resolved_paths {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(MEDIA_PREFIX);
        out.push_str(&path);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn touch(path: &PathBuf) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(path, b"test").unwrap();
    }

    #[test]
    fn strips_inline_media_token_when_file_exists() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-inline-{}.png",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("Hello MEDIA:{path} world"));
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains("MEDIA:"));
        assert_eq!(media, vec![path]);
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn keeps_inline_media_token_when_file_missing() {
        let path = format!(
            "/tmp/pointer-outbound-missing-inline-{}.png",
            uuid::Uuid::new_v4()
        );
        let (text, media) = split_reply_media(&format!("Hello MEDIA:{path} world"));
        assert!(text.contains("MEDIA:"));
        assert!(text.contains(&path));
        assert!(media.is_empty());
    }

    #[test]
    fn strips_media_lines_when_file_exists() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-line-{}.png",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("Hello\nMEDIA:{path}\nWorld"));
        assert_eq!(text, "Hello\nWorld");
        assert_eq!(media, vec![path]);
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn keeps_media_line_when_file_missing() {
        let path = format!(
            "/tmp/pointer-outbound-missing-line-{}.png",
            uuid::Uuid::new_v4()
        );
        let (text, media) = split_reply_media(&format!("Hello\nMEDIA:{path}\nWorld"));
        assert_eq!(text, format!("Hello\nMEDIA:{path}\nWorld"));
        assert!(media.is_empty());
    }

    #[test]
    fn strip_for_app_ui_keeps_unresolved_media_line() {
        let out = strip_outbound_media_markers(
            "找到了桌面上的 baby_cover.jpg，发给你 👇\n\nMEDIA:/Users/starliu/Desktop/baby_cover.jpg",
        );
        assert!(out.contains("MEDIA:"));
        assert!(out.contains("/Users/starliu/Desktop/baby_cover.jpg"));
        assert!(out.contains("发给你"));
        assert!(out.contains("baby_cover.jpg"));
    }

    #[test]
    fn bare_pointer_media_unresolved_stays_in_text() {
        let uri = "pointer-media://conv-id/27582151-a370-4170-873b-bc12b9bff2c5.wav";
        let (text, media) = split_reply_media(&format!("{uri}\n当然可以！我能帮你写代码。"));
        assert!(media.is_empty());
        assert!(text.contains("当然可以"));
        assert!(text.contains("pointer-media://"));
    }

    #[test]
    fn bare_pointer_media_resolved_is_outbound() {
        let root = crate::storage::app_data_dir().expect("app data dir");
        let rel = format!(
            "_anonymous/outbound-bare-{}/out.md",
            uuid::Uuid::new_v4()
        );
        let file = root.join("conversation-media").join(&rel);
        touch(&file);
        let uri = format!("pointer-media://{rel}");
        let (text, media) = split_reply_media(&format!("文档在这\n{uri}\n谢谢"));
        assert_eq!(media, vec![uri.clone()]);
        assert!(!text.contains("pointer-media://"));
        assert!(text.contains("文档在这"));
        assert!(text.contains("谢谢"));
        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir(file.parent().unwrap());
    }

    #[test]
    fn media_prefix_pointer_uri_is_outbound_when_file_exists() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-uri-{}.wav",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let uri = format!("file://{}", file.display());
        let (text, media) = split_reply_media(&format!("好的\nMEDIA:{uri}"));
        assert_eq!(media, vec![uri]);
        assert!(!text.contains("MEDIA:"));
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn spaced_path_on_own_line_resolves() {
        let dir = std::env::temp_dir().join(format!(
            "pointer app support {}",
            uuid::Uuid::new_v4()
        ));
        let file = dir.join("report v4.md");
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("MEDIA:{path}"));
        assert_eq!(media, vec![path]);
        assert!(!text.contains("MEDIA:"));
        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn spaced_path_inline_after_label_resolves() {
        let dir = std::env::temp_dir().join(format!(
            "pointer app support {}",
            uuid::Uuid::new_v4()
        ));
        let file = dir.join("report v4.md");
        touch(&file);
        let path = file.display().to_string();
        // Model-style delivery: label + MEDIA on the same bullet line (spaces in path).
        let reply = format!("- MD 文档 V4.0：MEDIA:{path}");
        let (text, media) = split_reply_media(&reply);
        assert_eq!(media, vec![path]);
        assert!(!text.contains("MEDIA:"));
        assert!(text.contains("MD 文档 V4.0"));
        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn spaced_path_missing_keeps_media_marker_in_text() {
        // Contract (5df5847): unresolved MEDIA must stay visible — never strip without attach.
        let path = format!(
            "/tmp/pointer app support {}/missing report.md",
            uuid::Uuid::new_v4()
        );
        let reply = format!("- MD 文档 V4.0：MEDIA:{path}");
        let (text, media) = split_reply_media(&reply);
        assert!(media.is_empty());
        assert!(text.contains("MEDIA:"));
        assert!(text.contains(&path));
        assert!(text.contains("MD 文档 V4.0"));
    }

    #[test]
    fn spaced_path_backtick_quoted_inline_resolves() {
        let dir = std::env::temp_dir().join(format!(
            "pointer app support {}",
            uuid::Uuid::new_v4()
        ));
        let file = dir.join("report v4.md");
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("文件：MEDIA:`{path}`"));
        assert_eq!(media, vec![path]);
        assert!(!text.contains("MEDIA:"));
        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn zero_byte_file_is_attached() {
        // Empty files are still files — size must not block MEDIA delivery.
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-empty-{}.txt",
            uuid::Uuid::new_v4()
        ));
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(&file, b"").unwrap();
        assert_eq!(fs::metadata(&file).unwrap().len(), 0);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!(
            "已创建空文件，内容为 0 字节：\nMEDIA:`{path}`\n如果你想换个文件名或目录，随时告诉我！"
        ));
        assert_eq!(media, vec![path]);
        assert!(!text.contains("MEDIA:"));
        assert!(text.contains("已创建空文件"));
        let _ = fs::remove_file(&file);
    }

    #[test]
    fn unclosed_backtick_still_attaches_when_file_exists() {
        // Models sometimes omit the closing backtick around Windows paths.
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-unclosed-{}.txt",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let (text, media) = split_reply_media(&format!("MEDIA:`{path}"));
        assert_eq!(media, vec![path]);
        assert!(!text.contains("MEDIA:"));
        let _ = fs::remove_file(&file);
    }

    /// Repro: sandbox empty.txt delivered with backticks (customer Windows path shape).
    ///
    /// Customer sample (file confirmed present on disk):
    /// `MEDIA:`C:\Users\…\PointerApp\session-sandboxes\1530c681-…\empty.txt``
    #[test]
    fn repro_pointerapp_sandbox_empty_txt_backtick_media() {
        let sandbox = std::env::temp_dir()
            .join(format!("PointerApp-repro-{}", uuid::Uuid::new_v4()))
            .join("session-sandboxes")
            .join("1530c681-176d-40ca-84b4-a90a34312628");
        let file = sandbox.join("empty.txt");
        fs::create_dir_all(&sandbox).unwrap();
        fs::write(&file, b"").unwrap();
        assert!(file.is_file());
        assert_eq!(fs::metadata(&file).unwrap().len(), 0);

        let path = file.display().to_string();
        let reply = format!(
            "已创建空文件，内容为 0 字节：\nMEDIA:`{path}`\n如果你想换个文件名或目录，随时告诉我！"
        );
        let (text, media) = split_reply_media(&reply);
        assert_eq!(
            media,
            vec![path.clone()],
            "expected MEDIA attach when file exists; visible text was:\n{text}"
        );
        assert!(
            !text.contains("MEDIA:"),
            "MEDIA marker should be stripped when file exists; text:\n{text}"
        );
        assert!(text.contains("已创建空文件"));
        assert!(text.contains("换个文件名"));

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(sandbox.parent().and_then(|p| p.parent()).unwrap_or(&sandbox));
    }

    /// Same prose as the customer report, but with a literal Windows-style absolute path
    /// string. On Windows the file is created at that location; on other OSes the path
    /// cannot exist, so MEDIA must stay visible (documents host-local resolve contract).
    #[test]
    fn repro_windows_absolute_sandbox_media_path_string() {
        let win_path = r"C:\Users\Administrator\AppData\Roaming\PointerApp\session-sandboxes\1530c681-176d-40ca-84b4-a90a34312628\empty.txt";
        let reply = format!(
            "已创建空文件，内容为 0 字节：\nMEDIA:`{win_path}`\n如果你想换个文件名或目录，随时告诉我！"
        );

        #[cfg(windows)]
        {
            let file = PathBuf::from(win_path);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&file, b"").unwrap();
            assert!(file.is_file());
            let (text, media) = split_reply_media(&reply);
            assert_eq!(
                media,
                vec![win_path.to_string()],
                "Windows host should attach existing sandbox empty.txt; text:\n{text}"
            );
            assert!(!text.contains("MEDIA:"));
            let _ = fs::remove_file(&file);
        }

        #[cfg(not(windows))]
        {
            let (text, media) = split_reply_media(&reply);
            assert!(
                media.is_empty(),
                "non-Windows host cannot see C:\\… paths; got media={media:?}"
            );
            assert!(
                text.contains("MEDIA:"),
                "unresolved Windows MEDIA must stay visible; text:\n{text}"
            );
            assert!(text.contains(win_path));
        }
    }

    #[test]
    fn im_outbound_reply_source_restores_resolved_media_from_raw() {
        let file = std::env::temp_dir().join(format!(
            "pointer-outbound-im-{}.html",
            uuid::Uuid::new_v4()
        ));
        touch(&file);
        let path = file.display().to_string();
        let raw = format!("文件在这里 👇\n\nMEDIA:{path}");
        let visible = strip_outbound_media_markers(&raw);
        let outbound = im_outbound_reply_source(Some(&raw), Some(&visible));
        let (text, media) = split_reply_media(&outbound);
        assert_eq!(text, visible);
        assert_eq!(media, vec![path]);
        let _ = fs::remove_file(&file);
    }
}
