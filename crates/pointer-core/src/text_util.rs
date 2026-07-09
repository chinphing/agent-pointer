//! UTF-8-safe string truncation helpers.
//!
//! Prefer these over manual `&s[..n]` slicing — byte indices can split multibyte
//! characters (CJK, emoji) and panic at runtime.

/// First `max_chars` Unicode scalars (no ellipsis).
pub fn take_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Truncate by scalar count; append `…` when shortened.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    format!("{}…", take_chars(s, max_chars))
}

/// Like [`truncate_chars`], but the result has at most `max_chars` scalars including `…`.
pub fn truncate_chars_fit(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let head = take_chars(s, max_chars.saturating_sub(1));
    format!("{head}…")
}

/// Truncate by UTF-8 byte budget without splitting a codepoint; append `…` when shortened.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", s[..end].trim_end())
}

/// Log-oriented scalar truncation with `(+N chars)` suffix.
pub fn truncate_for_log(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    let head = take_chars(s, max_chars);
    format!("{head}…(+{} chars)", n - max_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_bytes_does_not_split_utf8() {
        let s = "描".repeat(50);
        let out = truncate_bytes(&s, 80);
        assert!(out.ends_with('…'));
        assert!(std::str::from_utf8(out.trim_end_matches('…').as_bytes()).is_ok());
    }

    #[test]
    fn truncate_chars_counts_scalars() {
        let s = "描".repeat(10);
        let out = truncate_chars(&s, 5);
        assert_eq!(out, "描描描描描…");
    }

    #[test]
    fn truncate_chars_fit_includes_ellipsis_in_budget() {
        let s = "描".repeat(10);
        let out = truncate_chars_fit(&s, 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn take_chars_never_panics_on_cjk() {
        let s = "描述这张图片的内容，包括图形、文字、颜色、整体风格等。";
        let out = take_chars(s, 80);
        assert!(out.len() <= s.len());
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }
}
