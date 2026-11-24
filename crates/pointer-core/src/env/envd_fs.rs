//! envd `/files` 客户端：Docker 与 Remote 共用。
//!
//! 协议（E2B envd）：
//! - `GET|POST {envd}/files?path=<绝对路径>&username=…`，body 为原始字节
//! - 读支持 `Range`（206）；目录不能用 `/files` 列举

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// envd 默认数据面端口（容器内）。
pub const ENVD_PORT: u16 = 49983;
/// 文件接口用户：沙箱内 agent 以 root 跑。
pub const FILES_USER: &str = "root";
/// `open_reader` 每次 Range 块大小。
const READER_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// 指向一个已在跑的 envd 的文件客户端。
#[derive(Clone)]
pub(crate) struct EnvdFs {
    http: reqwest::blocking::Client,
    envd_url: String,
}

impl std::fmt::Debug for EnvdFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvdFs")
            .field("envd_url", &self.envd_url)
            .finish()
    }
}

impl EnvdFs {
    pub(crate) fn new(envd_url: impl Into<String>) -> Result<Self> {
        let envd_url = envd_url.into().trim_end_matches('/').to_string();
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("build envd http client")?;
        Ok(Self { http, envd_url })
    }

    pub(crate) fn envd_url(&self) -> &str {
        &self.envd_url
    }

    /// 轮询 `/health`，直到就绪或超时。
    pub(crate) fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut last = String::new();
        while Instant::now() < deadline {
            match self
                .http
                .get(format!("{}/health", self.envd_url))
                .timeout(Duration::from_secs(2))
                .send()
            {
                Ok(resp) if resp.status().is_success() => {
                    log::info!("envd ready url={}", self.envd_url);
                    return Ok(());
                }
                Ok(resp) => last = format!("HTTP {}", resp.status().as_u16()),
                Err(e) => last = format!("{e:#}"),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!(
            "envd 在 {:?} 内未就绪 url={} ({last})",
            timeout,
            self.envd_url
        )
    }

    fn files_url(&self, path: &Path) -> Result<String> {
        let p = path_to_str(path)?;
        Ok(format!(
            "{}/files?path={}&username={}",
            self.envd_url,
            urlencode(&p),
            FILES_USER
        ))
    }

    pub(crate) fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        super::run_blocking(|| {
            let resp = self
                .http
                .get(self.files_url(path)?)
                .timeout(Duration::from_secs(120))
                .send()
                .with_context(|| format!("read_file {}", path.display()))?;
            let status = resp.status();
            let bytes = resp.bytes().unwrap_or_default().to_vec();
            if !status.is_success() {
                bail!(
                    "read_file {} failed: HTTP {} {}",
                    path.display(),
                    status.as_u16(),
                    snippet(&bytes)
                );
            }
            Ok(bytes)
        })
    }

    pub(crate) fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        super::run_blocking(|| {
            let resp = self
                .http
                .post(self.files_url(path)?)
                .header("Content-Type", "application/octet-stream")
                .timeout(Duration::from_secs(300))
                .body(data.to_vec())
                .send()
                .with_context(|| format!("write_file {}", path.display()))?;
            let status = resp.status();
            if status.is_success() {
                return Ok(());
            }
            let body = resp.bytes().unwrap_or_default();
            bail!(
                "write_file {} failed: HTTP {} {}",
                path.display(),
                status.as_u16(),
                snippet(&body)
            );
        })
    }

    /// 按 Range 流式读。`total` 来自调用方的 `metadata().len`。
    pub(crate) fn open_reader(
        &self,
        path: &Path,
        total: u64,
    ) -> Result<Box<dyn std::io::Read + Send>> {
        let url = self.files_url(path)?;
        Ok(Box::new(RangeReader::new(self.http.clone(), url, total))
            as Box<dyn std::io::Read + Send>)
    }
}

/// 按 HTTP Range 分块拉取的只读流。
pub(crate) struct RangeReader {
    http: reqwest::blocking::Client,
    url: String,
    total: u64,
    offset: u64,
    buf: Vec<u8>,
    pos: usize,
    chunk: u64,
}

impl RangeReader {
    pub(crate) fn new(http: reqwest::blocking::Client, url: String, total: u64) -> Self {
        Self {
            http,
            url,
            total,
            offset: 0,
            buf: Vec::new(),
            pos: 0,
            chunk: READER_CHUNK_BYTES,
        }
    }

    fn fill(&mut self) -> std::io::Result<bool> {
        if self.offset >= self.total {
            return Ok(false);
        }
        let end = (self.offset + self.chunk - 1).min(self.total.saturating_sub(1));
        let resp = self
            .http
            .get(&self.url)
            .header("Range", format!("bytes={}-{}", self.offset, end))
            .timeout(Duration::from_secs(120))
            .send()
            .map_err(|e| std::io::Error::other(format!("range read failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(std::io::Error::other(format!(
                "range read failed: HTTP {}",
                status.as_u16()
            )));
        }
        let body = resp
            .bytes()
            .map_err(|e| std::io::Error::other(format!("range read body failed: {e}")))?
            .to_vec();
        if body.is_empty() {
            return Ok(false);
        }
        self.offset += body.len() as u64;
        self.buf = body;
        self.pos = 0;
        Ok(true)
    }
}

impl std::io::Read for RangeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() && !self.fill()? {
            return Ok(0);
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn path_to_str(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        bail!("envd 文件接口只接受绝对路径: {}", path.display());
    }
    Ok(path.to_string_lossy().into_owned())
}

fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn snippet(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    let clipped: String = trimmed.chars().take(400).collect();
    if trimmed.chars().count() > 400 {
        format!("{clipped}…")
    } else {
        clipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_keeps_slashes() {
        assert_eq!(urlencode("/data/a b"), "/data/a%20b");
    }

    #[test]
    fn rejects_relative_path() {
        assert!(path_to_str(Path::new("rel")).is_err());
    }
}
