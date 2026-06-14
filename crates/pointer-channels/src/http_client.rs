use anyhow::{Context, Result};
use parking_lot::Mutex;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
    tokens: Arc<Mutex<HashMap<String, CachedToken>>>,
}

#[derive(Clone)]
struct CachedToken {
    value: String,
    /// Wall-clock expiry (Unix seconds). Survives system sleep unlike `Instant`.
    expires_at_unix_secs: u64,
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
        let now = unix_secs_now();
        let guard = self.tokens.lock();
        guard.get(key).and_then(|t| {
            if t.expires_at_unix_secs > now {
                Some(t.value.clone())
            } else {
                None
            }
        })
    }

    pub fn invalidate_cached_token(&self, key: &str) {
        self.tokens.lock().remove(key);
    }

    pub fn set_cached_token(&self, key: &str, value: String, ttl_secs: u64) {
        self.set_cached_token_with_early_refresh(key, value, ttl_secs, 60);
    }

    pub fn set_cached_token_with_early_refresh(
        &self,
        key: &str,
        value: String,
        ttl_secs: u64,
        early_refresh_secs: u64,
    ) {
        let now = unix_secs_now();
        let margin = early_refresh_secs.min(ttl_secs.saturating_sub(1));
        let mut guard = self.tokens.lock();
        guard.insert(
            key.to_string(),
            CachedToken {
                value,
                expires_at_unix_secs: now.saturating_add(ttl_secs.saturating_sub(margin)),
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

    pub async fn post_multipart(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        parts: Vec<(&str, Vec<u8>, Option<String>)>,
    ) -> Result<Value> {
        let mut form = reqwest::multipart::Form::new();
        for (name, data, file_name) in parts {
            let part = if let Some(fname) = file_name {
                reqwest::multipart::Part::bytes(data).file_name(fname)
            } else {
                reqwest::multipart::Part::bytes(data)
            };
            form = form.part(name.to_string(), part);
        }
        let mut req = self.inner.post(url).multipart(form);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.context("http multipart post")?;
        let status = resp.status();
        let text = resp.text().await.context("http multipart body")?;
        if !status.is_success() {
            return Err(anyhow::anyhow!("POST {url} failed {status}: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    pub async fn post_bytes(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        content_type: &str,
    ) -> Result<(Value, Vec<(String, String)>)> {
        let mut req = self
            .inner
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body.to_vec());
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.context("http post bytes")?;
        let status = resp.status();
        let resp_headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                Some((k.as_str().to_string(), v.to_str().ok()?.to_string()))
            })
            .collect();
        let text = resp.text().await.context("http post bytes body")?;
        if !status.is_success() {
            return Err(anyhow::anyhow!("POST {url} failed {status}: {text}"));
        }
        let json = serde_json::from_str(&text).unwrap_or(Value::String(text));
        Ok((json, resp_headers))
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("http client")
    }
}

fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
