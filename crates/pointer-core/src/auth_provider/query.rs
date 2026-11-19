//! Query-string assembly for authorize URLs.
//!
//! Providers expect RFC 3986 percent-encoding of every parameter value; a raw
//! `redirect_uri` with its own `?`/`&` would otherwise split the query.

/// Percent-encode everything except RFC 3986 unreserved characters.
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Append `params` to `base`, keeping any query the base URL already carries.
/// Empty names and empty values are skipped.
pub fn with_params(base: &str, params: &[(&str, String)]) -> String {
    let mut url = base.trim().to_string();
    let mut separator = if url.contains('?') { '&' } else { '?' };
    for (name, value) in params {
        let name = name.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        url.push(separator);
        separator = '&';
        url.push_str(&percent_encode(name));
        url.push('=');
        url.push_str(&percent_encode(value));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_characters() {
        assert_eq!(
            percent_encode("https://a.example/cb?x=1&y=2"),
            "https%3A%2F%2Fa.example%2Fcb%3Fx%3D1%26y%3D2"
        );
        assert_eq!(percent_encode("a b"), "a%20b");
        // Unreserved characters stay literal.
        assert_eq!(percent_encode("aZ0-_.~"), "aZ0-_.~");
    }

    #[test]
    fn appends_and_skips_empty_values() {
        let url = with_params(
            "https://idp.example/auth",
            &[("response_type", "code".into()), ("empty", String::new())],
        );
        assert_eq!(url, "https://idp.example/auth?response_type=code");

        // Base URL that already has a query keeps a single `?`.
        let url = with_params("https://idp.example/auth?x=1", &[("state", "s".into())]);
        assert_eq!(url, "https://idp.example/auth?x=1&state=s");
    }
}
