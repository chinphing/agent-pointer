//! UTF-8-safe string truncation / slicing helpers.
//!
//! Prefer these over manual `&s[..n]` slicing — byte indices can split multibyte
//! characters (CJK, emoji) and panic at runtime.

/// Largest char boundary `<= index` (clamped to `0..=s.len()`).
pub fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut i = index.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary `>= index` (clamped to `0..=s.len()`).
pub fn ceil_char_boundary(s: &str, index: usize) -> usize {
    let mut i = index.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// UTF-8-safe `&s[start..end]` by byte offsets (clamps both ends to char boundaries).
pub fn slice_bytes(s: &str, start: usize, end: usize) -> &str {
    let start = ceil_char_boundary(s, start.min(s.len()));
    let end = floor_char_boundary(s, end.min(s.len()));
    if start >= end {
        return "";
    }
    &s[start..end]
}

/// Split at a byte offset without panicking (`mid` is floored to a char boundary).
pub fn split_at_byte(s: &str, mid: usize) -> (&str, &str) {
    let mid = floor_char_boundary(s, mid);
    (&s[..mid], &s[mid..])
}

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
    format!("{}…", slice_bytes(s, 0, max_bytes).trim_end())
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

/// Collapse runs of whitespace (including newlines) to single spaces for UI snippets.
pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `text` contains `query` as a contiguous substring (case-insensitive),
/// after whitespace collapse. Used to prefer FTS rows that can show a real hit.
pub fn text_contains_query(text: &str, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return false;
    }
    collapse_whitespace(text)
        .to_lowercase()
        .contains(&q.to_lowercase())
}

/// Build a short preview around the first case-insensitive match of `query`.
///
/// Prefer this over SQLite FTS5 `snippet()` for CJK / long bodies — FTS token
/// windows often return the document head instead of the hit.
///
/// The match is placed **near the start** of the snippet (small `before`, larger
/// `after`) so narrow UI with CSS `truncate` still shows the keyword. Optional
/// `mark_pre` / `mark_post` wrap the matched span (e.g. `<b>`).
pub fn match_centered_snippet(
    text: &str,
    query: &str,
    radius: usize,
    mark_pre: &str,
    mark_post: &str,
) -> String {
    // Bias: keep most of the budget after the hit so sidebar truncate shows it.
    let before = (radius / 4).clamp(4, 10);
    let after = radius.saturating_mul(2).saturating_sub(before).max(radius);
    match_window_snippet(text, query, before, after, mark_pre, mark_post)
}

/// At most `max_chars` around the first query hit (about 1/4 before, rest after).
/// No highlight markers. Empty query or no hit → document head.
pub fn match_centered_excerpt(text: &str, query: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if query.trim().is_empty() {
        return truncate_chars(text, max_chars);
    }
    let before = (max_chars / 4).max(1);
    let after = max_chars.saturating_sub(before).max(1);
    match_window_snippet(text, query, before, after, "", "")
}

fn match_window_snippet(
    text: &str,
    query: &str,
    before: usize,
    after: usize,
    mark_pre: &str,
    mark_post: &str,
) -> String {
    let flat = collapse_whitespace(text);
    if flat.is_empty() {
        return String::new();
    }
    let needles = snippet_needles(query);
    let lower_flat = flat.to_lowercase();
    let mut best: Option<(usize, usize)> = None; // (char_start, char_len)
    for needle in &needles {
        let lower_needle = needle.to_lowercase();
        if lower_needle.is_empty() {
            continue;
        }
        if let Some(byte_pos) = lower_flat.find(&lower_needle) {
            let char_start = flat[..byte_pos].chars().count();
            let char_len = needle.chars().count();
            // Prefer earlier match; among equal starts, prefer longer needle.
            best = Some(match best {
                Some((s, l)) if s < char_start || (s == char_start && l >= char_len) => (s, l),
                _ => (char_start, char_len),
            });
        }
    }
    let Some((match_start, match_len)) = best else {
        return truncate_chars_fit(&flat, before.saturating_add(after).saturating_add(1));
    };
    let total = flat.chars().count();
    let match_end = (match_start + match_len).min(total);
    let start = match_start.saturating_sub(before);
    let end = (match_end + after).min(total);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    for (i, ch) in flat.chars().enumerate() {
        if i < start {
            continue;
        }
        if i >= end {
            break;
        }
        if i == match_start && !mark_pre.is_empty() {
            out.push_str(mark_pre);
        }
        out.push(ch);
        if i + 1 == match_end && !mark_post.is_empty() {
            out.push_str(mark_post);
        }
    }
    if end < total {
        out.push('…');
    }
    out
}

fn snippet_needles(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    // Always try the full query first (contiguous phrase).
    out.push(trimmed.to_string());
    for term in trimmed.split_whitespace() {
        let t = term
            .trim_matches(|c| c == '"' || c == '*')
            .trim_start_matches('-')
            .to_string();
        if !t.is_empty()
            && !t.eq_ignore_ascii_case("OR")
            && !t.eq_ignore_ascii_case("AND")
            && t != trimmed
        {
            out.push(t);
        }
    }
    // Prefer longer needles first so multi-char CJK phrases win over fragments.
    out.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    out.dedup();
    out
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
    fn slice_bytes_and_split_at_byte_never_split_cjk() {
        let s = "可用 `MEDIA:` 取回本地";
        // Land inside `地` (last CJK char).
        let inside_di = s.len() - 1;
        assert!(!s.is_char_boundary(inside_di));
        let kept = slice_bytes(s, 0, inside_di);
        assert!(kept.ends_with('本') || kept.ends_with('回') || !kept.is_empty());
        let (head, tail) = split_at_byte(s, inside_di);
        assert_eq!(format!("{head}{tail}"), s);
        assert!(head.is_char_boundary(head.len()));
        assert!(tail.is_char_boundary(0));
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
    fn match_centered_snippet_includes_cjk_hit_not_doc_head() {
        let prefix = "=== 第1页 === - 1 - ".repeat(30);
        let text = format!("{prefix}工作城市「北京」已填写");
        let snip = match_centered_snippet(&text, "北京", 16, "", "");
        assert!(snip.contains("北京"), "snippet={snip}");
        assert!(!snip.starts_with("=== 第1页"), "snippet={snip}");
    }

    #[test]
    fn match_centered_snippet_keeps_hit_near_start_for_truncate() {
        // Long prefix before the hit: old symmetric window put the keyword near
        // the end, which CSS truncate then clipped away in the sidebar.
        let prefix = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".repeat(3);
        let text = format!("{prefix}工作城市「北京」后续说明文字很多");
        let snip = match_centered_snippet(&text, "北京", 16, "", "");
        assert!(snip.contains("北京"), "snippet={snip}");
        // Keyword should appear within the first ~20 visible chars (after …).
        let body = snip.trim_start_matches('…');
        let byte_pos = body.find("北京").expect("hit");
        let char_pos = body[..byte_pos].chars().count();
        assert!(
            char_pos <= 8,
            "hit too far right for narrow truncate: char_pos={char_pos}, snippet={snip}"
        );
    }

    #[test]
    fn match_centered_snippet_marks_hit() {
        let snip = match_centered_snippet("hello 北京 world", "北京", 8, "<b>", "</b>");
        assert!(snip.contains("<b>北京</b>"), "snippet={snip}");
    }

    #[test]
    fn match_centered_excerpt_keeps_hit_not_doc_head() {
        let prefix = "HEAD ".repeat(400);
        let text = format!("{prefix}工作城市「北京」后续说明");
        let out = match_centered_excerpt(&text, "北京", 40);
        assert!(out.contains("北京"), "excerpt={out}");
        assert!(!out.contains("HEAD HEAD"), "excerpt={out}");
        assert!(
            out.chars().count() <= 50,
            "excerpt_chars={}",
            out.chars().count()
        );
    }

    #[test]
    fn text_contains_query_collapses_whitespace() {
        assert!(text_contains_query("工作城市\n「北京」", "北京"));
        assert!(!text_contains_query("只有北 和 京分开", "北京"));
    }

    #[test]
    fn take_chars_never_panics_on_cjk() {
        let s = "描述这张图片的内容，包括图形、文字、颜色、整体风格等。";
        let out = take_chars(s, 80);
        assert!(out.len() <= s.len());
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }
}
