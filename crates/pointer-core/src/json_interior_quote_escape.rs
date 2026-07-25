//! Tools for fixing **ASCII `"`** inside JSON-like text.
//!
//! ## Interior-only ([`escape_unescaped_double_quotes_in_json_string_interior`])
//! Pass a **single string value’s interior** (no outer `"`). Uses the JSON rule: a `"` is
//! already escaped iff it is preceded by an **odd** number of consecutive `\` bytes.
//!
//! ## Whole-text heuristic ([`repair_json_unescaped_quotes`])
//! Walks the blob with `in_string` / `outside` state. Inside a string, a `"` with an even
//! backslash prefix is either a real closing quote or a stray interior quote; we treat it as a
//! **close** only when the following bytes look like normal JSON after a value or key (`...` `,`
//! `}`, `:`), or `]` only when a **structural** JSON array is open. A bare lookahead `"` (for
//! example in markdown `primary("")`) does **not** end the string. Empty JSON strings `""` still
//! work because the second quote’s lookahead is `,`, `}`, etc. Otherwise we emit `\"` and stay
//! inside the string.
//!
//! This is **best-effort** (ambiguous broken shapes exist); prefer valid model output + format
//! retry when repair is uncertain.
//!
//! **Literal newlines in strings:** [`escape_literal_newlines_in_json_strings`] and
//! [`munge_finalize_json_parse`] run only on the finalize path in `json_tool_caller`, not on
//! streaming partial extraction.
//!
//! **Rust literals:** embedding JSON that ends with a closing string quote immediately before `}`
//! needs a raw delimiter with extra hashes (for example `r##" … "##`), or the `r#" … "#` form
//! can terminate the literal early and omit that quote.

/// Counts consecutive `\` bytes immediately before `quote_byte_index` (0 if none).
fn count_backslashes_before_quote(bytes: &[u8], quote_byte_index: usize) -> usize {
    let mut c = 0usize;
    let mut j = quote_byte_index;
    while j > 0 && bytes[j - 1] == b'\\' {
        c += 1;
        j -= 1;
    }
    c
}

/// After a closing `"` at byte index `after_quote` (first byte *after* that `"`), returns true
/// when the following non-ASCII-whitespace byte looks like JSON may continue after a string token.
///
/// `structural_bracket_depth` counts `[` / `]` **outside** of JSON strings only. A literal `]` in
/// markdown (with no structural array open) must not force a premature string close.
fn looks_like_json_string_token_end(
    bytes: &[u8],
    mut after_quote: usize,
    structural_bracket_depth: usize,
) -> bool {
    while after_quote < bytes.len() && bytes[after_quote].is_ascii_whitespace() {
        after_quote += 1;
    }
    if after_quote >= bytes.len() {
        return true;
    }
    match bytes[after_quote] {
        b',' | b'}' | b':' => true,
        b']' => structural_bracket_depth > 0,
        _ => false,
    }
}

/// Escapes interior `"` that are not already part of a valid `\"` sequence (odd count of `\`
/// immediately before the quote).
///
/// Not yet called from the parse path; keep available for bounded `tool_args.*` repair attempts.
#[allow(dead_code)]
pub(crate) fn escape_unescaped_double_quotes_in_json_string_interior(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(8));
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if count_backslashes_before_quote(bytes, i) % 2 == 0 {
                out.push('\\');
            }
            out.push('"');
            i += 1;
        } else {
            let tail = &s[i..];
            let ch = tail.chars().next().expect("i < len implies non-empty tail");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Best-effort repair of **unescaped** `"` inside JSON **string values** while walking a full
/// document. Valid JSON is preserved in typical cases; see module docs for limits.
pub(crate) fn repair_json_unescaped_quotes(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len().saturating_add(16));
    let mut in_string = false;
    let mut structural_bracket_depth = 0usize;
    let mut it = input.char_indices().peekable();

    while let Some((i, c)) = it.next() {
        if !in_string {
            match c {
                '[' => {
                    structural_bracket_depth += 1;
                    out.push('[');
                }
                ']' => {
                    structural_bracket_depth = structural_bracket_depth.saturating_sub(1);
                    out.push(']');
                }
                '"' => {
                    in_string = true;
                    out.push('"');
                }
                _ => out.push(c),
            }
            continue;
        }

        if c == '"' {
            let bs = count_backslashes_before_quote(bytes, i);
            if bs % 2 == 1 {
                out.push('"');
                continue;
            }
            let after = i + c.len_utf8();
            if looks_like_json_string_token_end(bytes, after, structural_bracket_depth) {
                in_string = false;
                out.push('"');
            } else {
                out.push('\\');
                out.push('"');
            }
            continue;
        }

        if c == '\\' {
            out.push('\\');
            match it.next() {
                Some((_, 'u')) => {
                    out.push('u');
                    for _ in 0..4 {
                        match it.next() {
                            Some((_, hex)) => out.push(hex),
                            None => break,
                        }
                    }
                }
                Some((_, next)) => out.push(next),
                None => {}
            }
            continue;
        }

        out.push(c);
    }

    out
}

/// Replaces **literal** ASCII control characters inside JSON **string tokens** only:
/// `\n` → `\\n`, `\r\n` / bare `\r` → `\\n` / `\\r`, `\t` → `\\t`.
/// Pretty-printed newlines **outside** strings are left unchanged.
///
/// Used only from [`crate::json_tool_caller::parse_tool_json_value`] (finalize path), not streaming.
pub(crate) fn escape_literal_newlines_in_json_strings(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len().saturating_add(16));
    let mut in_string = false;
    let mut structural_bracket_depth = 0usize;
    let mut it = input.char_indices().peekable();

    while let Some((i, c)) = it.next() {
        if !in_string {
            match c {
                '[' => {
                    structural_bracket_depth += 1;
                    out.push('[');
                }
                ']' => {
                    structural_bracket_depth = structural_bracket_depth.saturating_sub(1);
                    out.push(']');
                }
                '"' => {
                    in_string = true;
                    out.push('"');
                }
                _ => out.push(c),
            }
            continue;
        }

        if c == '"' {
            let bs = count_backslashes_before_quote(bytes, i);
            if bs % 2 == 1 {
                out.push('"');
                continue;
            }
            let after = i + c.len_utf8();
            if looks_like_json_string_token_end(bytes, after, structural_bracket_depth) {
                in_string = false;
                out.push('"');
            } else {
                out.push('\\');
                out.push('"');
            }
            continue;
        }

        if c == '\\' {
            out.push('\\');
            match it.next() {
                Some((_, 'u')) => {
                    out.push('u');
                    for _ in 0..4 {
                        match it.next() {
                            Some((_, hex)) => out.push(hex),
                            None => break,
                        }
                    }
                }
                Some((_, next)) => out.push(next),
                None => {}
            }
            continue;
        }

        match c {
            '\n' => out.push_str("\\n"),
            '\r' => {
                if it.peek().is_some_and(|&(_, ch)| ch == '\n') {
                    let _ = it.next();
                    out.push_str("\\n");
                } else {
                    out.push_str("\\r");
                }
            }
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }

    out
}

/// Finalize-only pipeline: escape literal newlines/tabs inside JSON strings, then apply
/// [`repair_json_unescaped_quotes`].
pub(crate) fn munge_finalize_json_parse(input: &str) -> String {
    repair_json_unescaped_quotes(&escape_literal_newlines_in_json_strings(input))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        escape_literal_newlines_in_json_strings,
        escape_unescaped_double_quotes_in_json_string_interior, munge_finalize_json_parse,
        repair_json_unescaped_quotes,
    };

    #[test]
    fn escape_literal_newlines_preserves_outside_string_whitespace() {
        let s = "{\n  \"a\": \"x\"\n}";
        assert_eq!(escape_literal_newlines_in_json_strings(s), s);
    }

    #[test]
    fn escape_literal_newlines_escapes_bare_lf_in_string() {
        let bad = concat!("{\"a\":\"p", "\n", "q\"}");
        let fixed = escape_literal_newlines_in_json_strings(bad);
        assert_eq!(fixed, r#"{"a":"p\nq"}"#);
        serde_json::from_str::<serde_json::Value>(&fixed).expect("fixed should parse");
    }

    #[test]
    fn munge_finalize_roundtrips_newline_then_quotes() {
        let bad = concat!(r#"{"text":"a"#, "\n", r#"b"}"#);
        let m = munge_finalize_json_parse(bad);
        serde_json::from_str::<serde_json::Value>(&m).expect("munge should parse");
    }

    #[test]
    fn interior_plain_quote_gets_backslash() {
        assert_eq!(
            escape_unescaped_double_quotes_in_json_string_interior(r#"He said "hi""#),
            r#"He said \"hi\""#
        );
    }

    #[test]
    fn interior_already_escaped_quote_unchanged() {
        assert_eq!(
            escape_unescaped_double_quotes_in_json_string_interior(r#"a\"b"#),
            r#"a\"b"#
        );
    }

    #[test]
    fn interior_utf8_around_ascii_quote() {
        assert_eq!(
            escape_unescaped_double_quotes_in_json_string_interior("い\"う"),
            "い\\\"う"
        );
    }

    #[test]
    fn interior_even_backslashes_before_quote_gets_escape() {
        assert_eq!(
            escape_unescaped_double_quotes_in_json_string_interior(r#"a\\"b"#),
            r#"a\\\"b"#
        );
    }

    #[test]
    fn serde_parses_expected_repaired_literal() {
        // `r##"... "##` so the JSON closing `"` before `}` is not swallowed by `r#"..."#` (`"#`).
        let s = r##"{"message": "小明说:\"我觉得Rust很好玩\"。"}"##;
        serde_json::from_str::<serde_json::Value>(s).expect("literal should parse");
    }

    #[test]
    fn repair_chinese_embedded_quotes_roundtrips_serde() {
        let bad = r##"{"message": "小明说:"我觉得Rust很好玩"。"}"##;
        let fixed = repair_json_unescaped_quotes(bad);
        let expected = r##"{"message": "小明说:\"我觉得Rust很好玩\"。"}"##;
        assert_eq!(fixed, expected, "repair output mismatch");
        let v: serde_json::Value =
            serde_json::from_str(&fixed).expect("repaired JSON should parse");
        assert_eq!(v["message"], json!("小明说:\"我觉得Rust很好玩\"\u{3002}"));
    }

    #[test]
    fn repair_valid_json_unchanged_simple() {
        let ok = r#"{"a":1,"b":"x","c":""}"#;
        assert_eq!(repair_json_unescaped_quotes(ok), ok);
        let v: serde_json::Value = serde_json::from_str(ok).unwrap();
        assert_eq!(v["b"], "x");
        assert_eq!(v["c"], "");
    }

    #[test]
    fn repair_preserves_escaped_quotes_inside_value() {
        let ok = r#"{"m":"say \"ok\" end"}"#;
        assert_eq!(repair_json_unescaped_quotes(ok), ok);
    }

    #[test]
    fn repair_preserves_unicode_escape() {
        let ok = r#"{"m":"\u0041"}"#;
        assert_eq!(repair_json_unescaped_quotes(ok), ok);
    }
}
