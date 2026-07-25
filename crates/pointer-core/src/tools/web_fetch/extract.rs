//! HTML → plain text / lightweight markdown for `web_fetch`.

use regex::Regex;
use std::sync::LazyLock;

static SCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("script strip regex"));
static STYLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("style strip regex"));
static NOSCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<noscript[^>]*>.*?</noscript>").expect("noscript strip regex")
});
static SVG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<svg[^>]*>.*?</svg>").expect("svg strip regex"));
static IFRAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<iframe[^>]*>.*?</iframe>").expect("iframe strip regex"));
static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").expect("comment strip regex"));
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").expect("tag strip regex"));
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title extract regex"));
static BLOCK_OPEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)</?(br|p|div|tr|li|h[1-6]|section|article|header|footer|main|ul|ol|table|blockquote)(\s[^>]*)?/?>",
    )
    .expect("block tag regex")
});

/// Convert HTML to readable plain text (lossy but good enough for LLM context).
pub fn html_to_text(html: &str) -> String {
    let mut s = SCRIPT_RE.replace_all(html, " ").to_string();
    s = STYLE_RE.replace_all(&s, " ").to_string();
    s = NOSCRIPT_RE.replace_all(&s, " ").to_string();
    s = SVG_RE.replace_all(&s, " ").to_string();
    s = IFRAME_RE.replace_all(&s, " ").to_string();
    s = COMMENT_RE.replace_all(&s, " ").to_string();
    s = BLOCK_OPEN_RE.replace_all(&s, "\n").to_string();
    s = TAG_RE.replace_all(&s, " ").to_string();
    s = html_unescape(&s);

    let mut out = String::with_capacity(s.len());
    let mut blank = 0usize;
    for line in s.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            blank += 1;
            if blank <= 2 {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&trimmed);
    }
    out.trim().to_string()
}

/// Prefer title + body text; fall back to raw when not HTML.
pub fn extract_content(body: &str, content_type: &str, mode: &str) -> (Option<String>, String) {
    let ct = content_type.to_ascii_lowercase();
    let looks_html = ct.contains("html")
        || body.trim_start().starts_with("<!DOCTYPE")
        || body.trim_start().starts_with("<html")
        || body.trim_start().starts_with("<HTML");

    if !looks_html {
        return (None, body.to_string());
    }

    let title = TITLE_RE
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| html_unescape(m.as_str()).trim().to_string())
        .filter(|t| !t.is_empty());

    let text = html_to_text(body);
    let content = if mode.eq_ignore_ascii_case("markdown") {
        match &title {
            Some(t) if !t.is_empty() => format!("# {t}\n\n{text}"),
            _ => text,
        }
    } else {
        text
    };

    (title, content)
}

fn html_unescape(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_and_tags() {
        let html = r#"<html><head><title>Hi</title><script>evil()</script></head>
            <body><p>Hello <b>world</b></p></body></html>"#;
        let (title, text) = extract_content(html, "text/html", "text");
        assert_eq!(title.as_deref(), Some("Hi"));
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains("evil"));
    }
}
