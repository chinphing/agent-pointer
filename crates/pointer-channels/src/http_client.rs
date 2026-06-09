use anyhow::{Context, Result};
use parking_lot::Mutex;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    tokens: Arc<Mutex<HashMap<String, CachedToken>>>,
}

#[derive(Clone)]
struct CachedToken {
    value: String,
    expires_at: Instant,
}

impl HttpClient {
    pub fn new() -> Result<Self> {
        let inner = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("reqwest client")?;
        Ok(Self {
            inner,
            tokens: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn client(&self) -> &Client {
        &self.inner
    }

    pub fn get_cached_token(&self, key: &str) -> Option<String> {
        let guard = self.tokens.lock();
        guard.get(key).and_then(|t| {
            if t.expires_at > Instant::now() {
                Some(t.value.clone())
            } else {
                None
            }
        })
    }

    pub fn set_cached_token(&self, key: &str, value: String, ttl_secs: u64) {
        let mut guard = self.tokens.lock();
        guard.insert(
            key.to_string(),
            CachedToken {
                value,
                expires_at: Instant::now() + Duration::from_secs(ttl_secs.saturating_sub(60)),
            },
        );
    }

    pub async fn get_json(&self, url: &str, headers: &[(&str, &str)]) -> Result<Value> {
        let mut req = self.inner.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.context("http get")?;
        let status = resp.status();
        let body = resp.text().await.context("http get body")?;
        if !status.is_success() {
            return Err(anyhow::anyhow!("GET {url} failed {status}: {body}"));
        }
        Ok(serde_json::from_str(&body).unwrap_or(Value::String(body)))
    }

    pub async fn get_bytes(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(Vec<u8>, Option<String>)> {
        let mut req = self.inner.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.context("http get bytes")?;
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
        let body = resp.bytes().await.context("http get bytes body")?;
        if !status.is_success() {
            let preview = String::from_utf8_lossy(&body[..body.len().min(512)]);
            return Err(anyhow::anyhow!(
                "GET {url} failed {status}: {preview}"
            ));
        }
        Ok((body.to_vec(), content_type))
    }

    pub async fn post_json(&self, url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value> {
        let mut req = self.inner.post(url).json(body);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.context("http post")?;
        let status = resp.status();
        let text = resp.text().await.context("http post body")?;
        if !status.is_success() {
            return Err(anyhow::anyhow!("POST {url} failed {status}: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    pub async fn post_json_typed<T: DeserializeOwned>(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &Value,
    ) -> Result<T> {
        let v = self.post_json(url, headers, body).await?;
        Ok(serde_json::from_value(v)?)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("http client")
    }
}
