//! Parse freedesktop `recently-used.xbel` for Linux recent apps (no launch counts).

use super::listed_app::{compare_listed_apps, ListedApp};
use chrono::NaiveDate;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parse XBEL text and return recent desktop apps within `cutoff` (newest first).
pub fn parse_xbel_recent_apps(content: &str, cutoff: NaiveDate) -> Vec<ListedApp> {
    let mut by_id: HashMap<String, ListedApp> = HashMap::new();

    for block in bookmark_blocks(content) {
        let Some(href) = attr_value(block, "href") else {
            continue;
        };
        let Some(desktop_id) = desktop_id_from_href(&href) else {
            continue;
        };
        let last_used = attr_value(block, "visited")
            .or_else(|| attr_value(block, "modified"))
            .or_else(|| attr_value(block, "added"))
            .and_then(|raw| parse_xbel_timestamp(&raw));
        let Some(last_used) = last_used else {
            continue;
        };
        if last_used < cutoff {
            continue;
        }

        let name = application_name(block)
            .or_else(|| title_text(block))
            .unwrap_or_else(|| desktop_id.replace('-', " "));

        let key = normalize_key(&desktop_id);
        let entry = by_id.entry(key).or_insert_with(|| ListedApp {
            name: name.clone(),
            identifier: desktop_id.clone(),
            running: false,
            frontmost: false,
            last_used: Some(last_used),
            uses: None,
            window_title: None,
            pid: None,
        });
        if entry.last_used.is_none_or(|d| last_used > d) {
            entry.last_used = Some(last_used);
            entry.name = name;
            entry.identifier = desktop_id;
        }
    }

    let mut entries: Vec<_> = by_id.into_values().collect();
    entries.sort_by(compare_listed_apps);
    entries
}

fn bookmark_blocks(content: &str) -> impl Iterator<Item = &str> {
    content.split("<bookmark ").skip(1).map(|rest| {
        rest.split_once("</bookmark>")
            .map(|(inner, _)| inner)
            .unwrap_or(rest)
    })
}

fn attr_value(block: &str, name: &str) -> Option<String> {
    let pattern = format!("{name}=\"");
    let start = block.find(&pattern)? + pattern.len();
    let tail = &block[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn application_name(block: &str) -> Option<String> {
    let marker = "bookmark:application";
    let idx = block.find(marker)?;
    let tail = &block[idx..];
    attr_value(tail, "name")
}

fn title_text(block: &str) -> Option<String> {
    let start = block.find("<title>")? + "<title>".len();
    let tail = &block[start..];
    let end = tail.find("</title>")?;
    let text = tail[..end].trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn desktop_id_from_href(href: &str) -> Option<String> {
    let path = href.strip_prefix("file://").unwrap_or(href);
    let path = url_decode(path);
    let path = Path::new(&path);
    if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
        return None;
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

fn url_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hi = chars.next();
            let lo = chars.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                if let Ok(byte) = u8::from_str_radix(&format!("{h}{l}"), 16) {
                    out.push(byte as char);
                    continue;
                }
            }
            out.push('%');
            if let Some(h) = hi {
                out.push(h);
            }
            if let Some(l) = lo {
                out.push(l);
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn parse_xbel_timestamp(raw: &str) -> Option<NaiveDate> {
    let trimmed = raw.trim();
    if trimmed.len() >= 10 {
        if let Ok(day) = NaiveDate::parse_from_str(&trimmed[..10], "%Y-%m-%d") {
            return Some(day);
        }
    }
    None
}

pub fn normalize_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[allow(dead_code)]
pub fn xbel_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("recently-used.xbel")
}

pub fn app_matches_identifier(app: &str, identifier: &str) -> bool {
    let needle = normalize_key(app);
    let hay = normalize_key(identifier);
    if needle.is_empty() || hay.is_empty() {
        return false;
    }
    let needle_stem = needle.strip_suffix(".desktop").unwrap_or(&needle);
    let hay_stem = hay.strip_suffix(".desktop").unwrap_or(&hay);
    hay.contains(&needle)
        || needle.contains(&hay)
        || hay_stem.contains(needle_stem)
        || needle_stem.contains(hay_stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    const SAMPLE_XBEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0">
  <bookmark href="file:///usr/share/applications/firefox.desktop" added="2026-05-01T10:00:00Z" visited="2026-06-10T12:00:00Z">
    <title>Firefox</title>
    <info>
      <metadata owner="http://freedesktop.org">
        <bookmark:application name="Firefox Web Browser" exec="'firefox' %u"/>
      </metadata>
    </info>
  </bookmark>
  <bookmark href="file:///usr/share/applications/org.gnome.Terminal.desktop" visited="2026-01-01T00:00:00Z">
    <title>Terminal</title>
  </bookmark>
</xbel>"#;

    #[test]
    fn parses_recent_desktop_apps() {
        let cutoff = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let apps = parse_xbel_recent_apps(SAMPLE_XBEL, cutoff);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].identifier, "firefox");
        assert_eq!(apps[0].name, "Firefox Web Browser");
        assert_eq!(apps[0].last_used, NaiveDate::from_ymd_opt(2026, 6, 10));
        assert!(apps[0].uses.is_none());
    }

    #[test]
    fn app_identifier_matching() {
        assert!(app_matches_identifier("firefox", "firefox"));
        assert!(app_matches_identifier("Firefox", "firefox.desktop"));
        assert!(!app_matches_identifier("chrome", "firefox"));
    }
}
