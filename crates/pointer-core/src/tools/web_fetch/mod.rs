//! `web_fetch` — HTTP GET public URLs and return readable text (Hermes `web_extract` / OpenClaw-style).

mod extract;
mod url_safety;

use self::url_safety::assert_url_safe;
use super::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, bail, Result};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use reqwest::redirect;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const DOC_SOURCE: &str = "tools/prompts/web_fetch.md";
const DOC: &str = include_str!("../prompts/web_fetch.md");

const DEFAULT_MAX_CHARS: usize = 50_000;
const MAX_CHARS_CAP: usize = 100_000;
const MAX_RESPONSE_BYTES: usize = 2_000_000;
const MAX_URLS: usize = 5;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_REDIRECTS: usize = 5;

const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

pub fn register_all(reg: &ToolRegistry) {
    let h: ToolHandler = Arc::new(|args| run_web_fetch(args));
    reg.register(
        ToolEntry::new("web_fetch", DOC_SOURCE, "medium", false, DOC, h)
            .with_subagent_inheritance(true),
    );
}

fn run_web_fetch(args: Value) -> Result<String> {
    // `reqwest::blocking` installs its own Tokio runtime. Calling it on a Tokio
    // worker (registry invoke from the chat loop) panics on drop:
    // "Cannot drop a runtime in a context where blocking is not allowed."
    // Run on a dedicated OS thread so we are outside any async runtime.
    let allow_loopback = url_safety::loopback_allowed();
    let handle = std::thread::Builder::new()
        .name("web_fetch".into())
        .spawn(move || {
            let _loopback = url_safety::enter_loopback_allowance_if(allow_loopback);
            run_web_fetch_blocking(args)
        })
        .map_err(|e| anyhow!("spawn web_fetch thread: {e}"))?;
    match handle.join() {
        Ok(result) => result,
        Err(panic) => {
            let msg = panic_payload_string(panic);
            log::error!("web_fetch: worker thread panicked: {msg}");
            Err(anyhow!("web_fetch panicked: {msg}"))
        }
    }
}

fn panic_payload_string(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn run_web_fetch_blocking(args: Value) -> Result<String> {
    let urls = parse_urls(&args)?;
    let max_chars = parse_max_chars(&args);
    let extract_mode = args
        .get("extractMode")
        .or_else(|| args.get("extract_mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("markdown")
        .trim()
        .to_ascii_lowercase();
    if extract_mode != "markdown" && extract_mode != "text" {
        bail!("extractMode must be \"markdown\" or \"text\"");
    }

    let client = build_client()?;
    let mut pages = Vec::with_capacity(urls.len());
    let mut any_ok = false;
    for url in &urls {
        match fetch_one(&client, url, max_chars, &extract_mode) {
            Ok(page) => {
                any_ok = true;
                pages.push(page);
            }
            Err(e) => {
                log::warn!("web_fetch: failed url={url}: {e:#}");
                pages.push(json!({
                    "ok": false,
                    "url": url,
                    "error": e.to_string(),
                }));
            }
        }
    }

    Ok(json!({
        "ok": any_ok,
        "pages": pages,
        "maxChars": max_chars,
    })
    .to_string())
}

fn parse_urls(args: &Value) -> Result<Vec<String>> {
    let mut out = Vec::new();
    if let Some(u) = args
        .get("url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        out.push(u.to_string());
    }
    if let Some(arr) = args.get("urls").and_then(|v| v.as_array()) {
        for item in arr {
            let s = item
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("urls entries must be non-empty strings"))?;
            if !out.iter().any(|x| x == s) {
                out.push(s.to_string());
            }
        }
    }
    if out.is_empty() {
        bail!("provide url or urls");
    }
    if out.len() > MAX_URLS {
        bail!("at most {MAX_URLS} URLs per call");
    }
    Ok(out)
}

fn parse_max_chars(args: &Value) -> usize {
    args.get("maxChars")
        .or_else(|| args.get("max_chars"))
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(1_000, MAX_CHARS_CAP))
        .unwrap_or(DEFAULT_MAX_CHARS)
}

fn build_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(CHROME_UA));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,text/plain;q=0.8,*/*;q=0.5",
        ),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

    Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .connect_timeout(Duration::from_secs(10))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.stop();
            }
            match assert_url_safe(attempt.url().as_str()) {
                Ok(_) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        }))
        .default_headers(headers)
        .build()
        .map_err(|e| anyhow!("http client: {e}"))
}

fn fetch_one(
    client: &Client,
    raw_url: &str,
    max_chars: usize,
    extract_mode: &str,
) -> Result<Value> {
    let url = assert_url_safe(raw_url)?;
    log::info!("web_fetch: GET {}", url.as_str());
    let resp = client
        .get(url.clone())
        .send()
        .map_err(|e| anyhow!("request failed: {e}"))?;
    let status = resp.status().as_u16();
    let final_url = resp.url().clone();
    assert_url_safe(final_url.as_str())?;
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.bytes().map_err(|e| anyhow!("read body failed: {e}"))?;
    let truncated_download = bytes.len() > MAX_RESPONSE_BYTES;
    let slice = if truncated_download {
        &bytes[..MAX_RESPONSE_BYTES]
    } else {
        &bytes
    };
    let raw = String::from_utf8_lossy(slice).to_string();
    let (content, content_kind, truncated_chars) =
        extract_content(&raw, &content_type, extract_mode, max_chars);

    Ok(json!({
        "ok": (200..300).contains(&status),
        "url": raw_url,
        "finalUrl": final_url.as_str(),
        "status": status,
        "contentType": content_type,
        "contentKind": content_kind,
        "content": content,
        "truncated": truncated_download || truncated_chars,
        "bytesRead": bytes.len().min(MAX_RESPONSE_BYTES),
    }))
}

fn extract_content(
    raw: &str,
    content_type: &str,
    extract_mode: &str,
    max_chars: usize,
) -> (String, &'static str, bool) {
    let ct = content_type.to_ascii_lowercase();
    let looks_html = ct.contains("text/html")
        || ct.contains("application/xhtml")
        || raw.trim_start().starts_with("<!DOCTYPE html")
        || raw.trim_start().starts_with("<html");

    let (text, kind) = if ct.contains("application/json") || ct.contains("+json") {
        (
            match serde_json::from_str::<Value>(raw) {
                Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| raw.to_string()),
                Err(_) => raw.to_string(),
            },
            "json",
        )
    } else if looks_html {
        let (_title, body) = extract::extract_content(raw, content_type, extract_mode);
        (body, "html_text")
    } else if ct.starts_with("text/") || ct.is_empty() {
        (raw.to_string(), "text")
    } else if raw
        .chars()
        .take(200)
        .all(|c| !c.is_control() || c.is_whitespace())
    {
        (raw.to_string(), "text")
    } else {
        (
            format!(
                "[binary or unsupported content-type: {content_type}; {} bytes]",
                raw.len()
            ),
            "unsupported",
        )
    };

    truncate_chars(&text, max_chars, kind)
}

fn truncate_chars(
    text: &str,
    max_chars: usize,
    kind: &'static str,
) -> (String, &'static str, bool) {
    let count = text.chars().count();
    if count <= max_chars {
        return (text.to_string(), kind, false);
    }
    let truncated: String = text.chars().take(max_chars).collect();
    (
        format!("{truncated}\n\n...[content truncated at {max_chars} chars]"),
        kind,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parse_urls_accepts_url_and_urls() {
        let urls = parse_urls(&json!({
            "url": "https://example.com/a",
            "urls": ["https://example.com/b", "https://example.com/a"]
        }))
        .unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/a");
        assert_eq!(urls[1], "https://example.com/b");
    }

    #[test]
    fn parse_urls_requires_at_least_one() {
        assert!(parse_urls(&json!({})).is_err());
    }

    #[tokio::test]
    async fn fetches_html_and_extracts_text() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/page"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                b"<html><body><h1>Title</h1><p>Hello fetch</p></body></html>".as_slice(),
                "text/html; charset=utf-8",
            ))
            .mount(&server)
            .await;

        let uri = format!("{}/page", server.uri());
        let out = tokio::task::spawn_blocking(move || {
            let _allow = url_safety::AllowLoopbackGuard::enter();
            run_web_fetch(json!({ "url": uri, "maxChars": 5000 }))
        })
        .await
        .unwrap()
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"], true);
        let content = v["pages"][0]["content"].as_str().unwrap();
        assert!(content.contains("Title"));
        assert!(content.contains("Hello fetch"));
    }

    #[test]
    fn redirect_target_localhost_is_rejected_by_safety() {
        let err = url_safety::assert_url_safe("http://127.0.0.1/secret")
            .expect_err("loopback must be blocked");
        assert!(err.to_string().contains("127.0.0.1") || err.to_string().contains("not allowed"));
    }
}
