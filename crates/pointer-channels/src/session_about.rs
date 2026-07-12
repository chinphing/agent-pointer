//! `/about` / `/version` command for IM channels — returns app version.

use pointer_core::client_env;

/// Build the about reply text (e.g. "Pointer v0.1.9").
pub fn about_text() -> String {
    format!("Pointer v{}", client_env::app_version())
}

/// Detect `/about`, `/version`, 关于, 版本 in inbound text.
pub fn is_about_command(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    for cmd in ["/about", "/version"] {
        if lower == cmd {
            return true;
        }
    }

    let cn = trimmed.trim_end_matches(['。', '！', '!', '？', '?', '.', ' ']);
    matches!(cn, "关于" | "版本")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_slash_about() {
        assert!(is_about_command("/about"));
        assert!(is_about_command("/ABOUT"));
        assert!(is_about_command("/version"));
        assert!(is_about_command("/VERSION"));
    }

    #[test]
    fn detects_chinese_about() {
        assert!(is_about_command("关于"));
        assert!(is_about_command("版本"));
        assert!(is_about_command("版本！"));
    }

    #[test]
    fn non_about_not_detected() {
        assert!(!is_about_command("关于这个"));
        assert!(!is_about_command("版本号"));
        assert!(!is_about_command("hello"));
        assert!(!is_about_command("/stop"));
    }

    #[test]
    fn about_text_has_version() {
        let text = about_text();
        assert!(text.starts_with("Pointer v"), "got: {text}");
        assert!(!text.ends_with('.'), "got: {text}");
    }
}
