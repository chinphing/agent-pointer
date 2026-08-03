//! Temporary Aliyun OSS upload for large video understanding (HTTP `video_url` to DashScope).
//!
//! Official API references:
//! - PutObject: <https://help.aliyun.com/zh/oss/developer-reference/putobject>
//! - V4 presigned URL: <https://help.aliyun.com/zh/oss/developer-reference/add-signatures-to-urls>
//! - V4 signature upgrade: <https://help.aliyun.com/zh/oss/developer-reference/guidelines-for-upgrading-v1-signatures-to-v4-signatures>

use super::filename::safe_attachment_basename;
use super::store::save_attachment_bytes;
use super::video::{shrink_video_to_max, COMPOSER_VIDEO_ADVISORY_BYTES};
use crate::models::MediaOssConfig;
use anyhow::{Context, Result};
use bytes::Bytes;
use chrono::Local;
use futures_util::Stream;
use ossify::ops::object::base::{
    DeleteObjectOperations, GetObjectOperations, GetObjectParams, PutObjectOperations,
    PutObjectOptions,
};
use ossify::{Client, QueryAuthOptions};
use std::env;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

/// Chunk size for streaming PutObject progress (512 KiB).
const UPLOAD_CHUNK_BYTES: usize = 512 * 1024;

/// Progress denominator — `loaded`/`total` map directly to 0–100% in the UI.
const PROGRESS_TOTAL: u64 = 100;
/// PutObject / compress phases stay at or below this; 100% only after presign succeeds.
const PUT_PHASE_END: u64 = 99;

struct ChunkUploadStream {
    chunks: Vec<Bytes>,
    idx: usize,
    phase_start: u64,
    phase_end: u64,
    upload_total: u64,
    sent: u64,
    on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>,
}

impl Stream for ChunkUploadStream {
    type Item = Result<Bytes, std::convert::Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.idx >= self.chunks.len() {
            return Poll::Ready(None);
        }
        let chunk = self.chunks[self.idx].clone();
        self.idx += 1;
        self.sent = self.sent.saturating_add(chunk.len() as u64);
        let span = self.phase_end.saturating_sub(self.phase_start);
        let mapped = self.phase_start + span.saturating_mul(self.sent) / self.upload_total.max(1);
        (self.on_progress)(mapped.min(self.phase_end), PROGRESS_TOTAL);
        Poll::Ready(Some(Ok(chunk)))
    }
}

fn report_progress(
    on_progress: &std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>,
    phase_start: u64,
    phase_end: u64,
    loaded: u64,
    phase_total: u64,
) {
    let span = phase_end.saturating_sub(phase_start);
    let mapped = phase_start + span.saturating_mul(loaded) / phase_total.max(1);
    on_progress(mapped.min(phase_end), PROGRESS_TOTAL);
}

async fn put_object_with_chunk_progress(
    client: &Client,
    object_key: &str,
    upload_bytes: &[u8],
    mime: &str,
    phase_start: u64,
    phase_end: u64,
    on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<()> {
    let upload_total = upload_bytes.len() as u64;
    on_progress(phase_start, PROGRESS_TOTAL);
    let chunks: Vec<Bytes> = upload_bytes
        .chunks(UPLOAD_CHUNK_BYTES)
        .map(Bytes::copy_from_slice)
        .collect();
    if chunks.is_empty() {
        on_progress(phase_end, PROGRESS_TOTAL);
        return Ok(());
    }
    let stream = ChunkUploadStream {
        chunks,
        idx: 0,
        phase_start,
        phase_end,
        upload_total,
        sent: 0,
        on_progress,
    };
    let options = PutObjectOptions::default().content_type(mime);
    client
        .put_object_stream(object_key, stream, Some(options))
        .await
        .context("OSS PutObject stream composer video")?;
    Ok(())
}

/// Resolved OSS credentials and endpoint (settings + environment).
#[derive(Debug, Clone)]
pub struct ResolvedMediaOssConfig {
    pub bucket: String,
    pub region: String,
    pub endpoint: String,
    pub access_key_id: String,
    pub access_key_secret: String,
    pub key_prefix: String,
    pub presign_expires_sec: u32,
    pub delete_after_use: bool,
}

pub struct OssTempVideo {
    pub object_key: String,
    pub read_url: String,
}

#[derive(Debug, Clone)]
pub struct ComposerVideoUploadResult {
    pub object_key: String,
    pub remote_url: String,
    /// Local conversation-media copy for preview and ffmpeg fallback.
    pub storage_rel_path: Option<String>,
}

fn attachment_key_prefix(resolved: &ResolvedMediaOssConfig) -> String {
    let p = resolved.key_prefix.trim();
    if p.is_empty() || p.contains("temp") {
        "pointer-media-attachments/".to_string()
    } else if p.ends_with('/') {
        p.to_string()
    } else {
        format!("{p}/")
    }
}

/// `YYYYMM` segment for OSS lifecycle cleanup by month.
fn oss_month_segment() -> String {
    Local::now().format("%Y%m").to_string()
}

fn attachment_object_key(prefix: &str, attachment_id: &str, file_name: &str) -> String {
    let safe = sanitize_object_name(file_name);
    format!(
        "{}{}/{}/{}",
        prefix,
        oss_month_segment(),
        attachment_id,
        safe
    )
}

fn env_first(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| env::var(k).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_enabled() -> bool {
    env_first(&["POINTER_MEDIA_OSS_ENABLED", "OSS_MEDIA_UPLOAD_ENABLED"])
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// Merge persisted settings with standard Aliyun/OSS environment variables.
pub fn resolve_media_oss_config(config: &MediaOssConfig) -> Option<ResolvedMediaOssConfig> {
    let enabled = config.enabled || env_enabled();
    if !enabled {
        log::warn!("media OSS: not configured — disabled in settings and env");
        return None;
    }

    let bucket = env_first(&["OSS_BUCKET", "POINTER_OSS_BUCKET"])
        .unwrap_or_else(|| config.bucket.trim().to_string());
    let region = env_first(&["OSS_REGION", "POINTER_OSS_REGION"])
        .unwrap_or_else(|| config.region.trim().to_string());
    if bucket.is_empty() || region.is_empty() {
        log::warn!(
            "media OSS: not configured — bucket or region missing (bucket_empty={} region_empty={})",
            bucket.is_empty(),
            region.is_empty()
        );
        return None;
    }

    let access_key_id = env_first(&[
        "OSS_ACCESS_KEY_ID",
        "ALIBABA_CLOUD_ACCESS_KEY_ID",
        "POINTER_OSS_ACCESS_KEY_ID",
    ])
    .unwrap_or_else(|| config.access_key_id.trim().to_string());
    let access_key_secret = env_first(&[
        "OSS_ACCESS_KEY_SECRET",
        "ALIBABA_CLOUD_ACCESS_KEY_SECRET",
        "POINTER_OSS_ACCESS_KEY_SECRET",
    ])
    .unwrap_or_else(|| config.access_key_secret.trim().to_string());
    if access_key_id.is_empty() || access_key_secret.is_empty() {
        log::warn!("media OSS: enabled but AccessKey id/secret missing");
        return None;
    }

    let endpoint = env_first(&["OSS_ENDPOINT", "POINTER_OSS_ENDPOINT"])
        .unwrap_or_else(|| config.endpoint.trim().to_string());
    let endpoint = if endpoint.is_empty() {
        format!("https://oss-{region}.aliyuncs.com")
    } else if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint
    } else {
        format!("https://{endpoint}")
    };

    let key_prefix = env_first(&["OSS_KEY_PREFIX", "POINTER_OSS_KEY_PREFIX"])
        .unwrap_or_else(|| config.key_prefix.trim().to_string());
    let key_prefix = if key_prefix.is_empty() {
        "pointer-media-temp/".to_string()
    } else if key_prefix.ends_with('/') {
        key_prefix
    } else {
        format!("{key_prefix}/")
    };

    let presign_expires_sec =
        env_first(&["OSS_PRESIGN_EXPIRES_SEC", "POINTER_OSS_PRESIGN_EXPIRES_SEC"])
            .and_then(|v| v.parse().ok())
            .filter(|&n: &u32| n >= 1 && n <= 604_800)
            .unwrap_or(config.presign_expires_sec.max(1).min(604_800));

    let delete_after_use = env_first(&["POINTER_OSS_DELETE_AFTER_USE"])
        .map(|v| !matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "NO"))
        .unwrap_or(config.delete_after_use);

    Some(ResolvedMediaOssConfig {
        bucket,
        region,
        endpoint,
        access_key_id,
        access_key_secret,
        key_prefix,
        presign_expires_sec,
        delete_after_use,
    })
}

fn build_client(resolved: &ResolvedMediaOssConfig) -> Result<Client> {
    build_client_with_timeout(resolved, Duration::from_secs(300))
}

/// Scale HTTP timeout from payload size (ossify default is 30s — too short for Composer videos).
fn oss_http_timeout_for_bytes(bytes: usize) -> Duration {
    let secs = (bytes as u64).saturating_div(256 * 1024).clamp(300, 7200);
    Duration::from_secs(secs)
}

fn build_client_with_timeout(
    resolved: &ResolvedMediaOssConfig,
    timeout: Duration,
) -> Result<Client> {
    Client::builder()
        .endpoint(&resolved.endpoint)
        .region(&resolved.region)
        .bucket(&resolved.bucket)
        .access_key_id(&resolved.access_key_id)
        .access_key_secret(&resolved.access_key_secret)
        .http_timeout(timeout)
        .build()
        .context("build OSS client")
}

async fn presign_composer_read_url(
    client: &Client,
    resolved: &ResolvedMediaOssConfig,
    object_key: &str,
) -> Result<String> {
    let auth_opts = QueryAuthOptions::builder()
        .x_oss_expires(resolved.presign_expires_sec)
        .build();
    let read_url = client
        .presign_get_object(object_key, true, GetObjectParams::new(), None, auth_opts)
        .await
        .context("OSS presign GET URL for composer video")?;
    if !read_url.starts_with("https://") {
        anyhow::bail!("OSS presigned URL must be HTTPS for DashScope video_url");
    }
    Ok(read_url)
}

fn sanitize_object_name(file_name: &str) -> String {
    let safe = safe_attachment_basename(file_name);
    if safe.is_empty() {
        "video.mp4".to_string()
    } else {
        safe
    }
}

fn temp_object_key(prefix: &str, file_name: &str) -> String {
    let safe = sanitize_object_name(file_name);
    let p = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    format!(
        "{}{}/{}/{}",
        p,
        oss_month_segment(),
        uuid::Uuid::new_v4(),
        safe
    )
}

/// Upload a Composer video attachment to OSS (public-read URL for API manifest / DashScope).
pub async fn upload_composer_video_bytes(
    config: &MediaOssConfig,
    attachment_id: &str,
    bytes: &[u8],
    file_name: &str,
    content_type: &str,
    compress: bool,
    conversation_id: Option<&str>,
    progress_base: u64,
    on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<ComposerVideoUploadResult> {
    log::info!(
        "composer video OSS bytes: attachment={attachment_id} file={file_name} bytes={} compress={compress}",
        bytes.len()
    );
    let resolved = resolve_media_oss_config(config).context("OSS not configured")?;
    if bytes.is_empty() {
        anyhow::bail!("cannot upload empty video to OSS");
    }
    on_progress(progress_base, PROGRESS_TOTAL);

    let storage_rel_path = match conversation_id.filter(|c| !c.trim().is_empty()) {
        Some(conv_id) => {
            let rel = save_attachment_bytes(conv_id, attachment_id, bytes, file_name)
                .context("save composer video local backup")?;
            log::info!(
                "composer video OSS: saved local backup attachment={attachment_id} rel={rel}"
            );
            Some(rel)
        }
        None => None,
    };

    let upload_bytes = if compress && bytes.len() > COMPOSER_VIDEO_ADVISORY_BYTES {
        log::info!(
            "composer video {file_name}: user confirmed compress ({:.1} MB → ≤{:.1} MB)",
            bytes.len() as f64 / (1024.0 * 1024.0),
            COMPOSER_VIDEO_ADVISORY_BYTES as f64 / (1024.0 * 1024.0),
        );
        let compress_end = progress_base.saturating_add(5).min(99);
        report_progress(&on_progress, progress_base, compress_end, 0, 1);
        let (out, report) = shrink_video_to_max(bytes, file_name, COMPOSER_VIDEO_ADVISORY_BYTES)
            .context("compress composer video before OSS upload")?;
        if report.changed() {
            log::info!(
                "composer video {file_name}: compressed {:.1} MB → {:.1} MB via {:?}",
                report.original_bytes as f64 / (1024.0 * 1024.0),
                report.final_bytes as f64 / (1024.0 * 1024.0),
                report.applied,
            );
        }
        on_progress(compress_end, PROGRESS_TOTAL);
        out
    } else {
        bytes.to_vec()
    };
    let upload_start = if compress && bytes.len() > COMPOSER_VIDEO_ADVISORY_BYTES {
        progress_base.saturating_add(5).min(99)
    } else {
        progress_base
    };
    let client =
        build_client_with_timeout(&resolved, oss_http_timeout_for_bytes(upload_bytes.len()))?;
    let prefix = attachment_key_prefix(&resolved);
    let object_key = attachment_object_key(&prefix, attachment_id, file_name);
    let mime = content_type.trim();
    let mime = if mime.is_empty() { "video/mp4" } else { mime };

    log::info!(
        "media OSS composer PutObject: bucket={} key={} bytes={} content-type={mime} timeout_sec={}",
        resolved.bucket,
        object_key,
        upload_bytes.len(),
        oss_http_timeout_for_bytes(upload_bytes.len()).as_secs()
    );

    put_object_with_chunk_progress(
        &client,
        &object_key,
        &upload_bytes,
        mime,
        upload_start,
        PUT_PHASE_END,
        on_progress.clone(),
    )
    .await
    .with_context(|| {
        format!(
            "OSS PutObject composer video (bucket={}, key={})",
            resolved.bucket, object_key
        )
    })?;
    on_progress(PUT_PHASE_END, PROGRESS_TOTAL);

    let remote_url = presign_composer_read_url(&client, &resolved, &object_key).await?;
    on_progress(PROGRESS_TOTAL, PROGRESS_TOTAL);
    log::info!("media OSS composer: presigned URL ready for {object_key}");
    Ok(ComposerVideoUploadResult {
        object_key,
        remote_url,
        storage_rel_path,
    })
}

/// Upload from a local path (Tauri file picker) with byte progress during read + upload.
pub async fn upload_composer_video_from_path(
    config: &MediaOssConfig,
    attachment_id: &str,
    path: &std::path::Path,
    file_name: &str,
    content_type: &str,
    compress: bool,
    conversation_id: Option<&str>,
    on_progress: std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<ComposerVideoUploadResult> {
    log::info!(
        "composer video OSS path: attachment={attachment_id} path={} file={file_name} compress={compress}",
        path.display()
    );
    let meta = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("stat video file {}", path.display()))?;
    let _file_size = meta.len();
    on_progress(0, PROGRESS_TOTAL);
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("read video file {}", path.display()))?;
    const READ_END: u64 = 10;
    on_progress(READ_END, PROGRESS_TOTAL);

    let storage_rel_path = match conversation_id.filter(|c| !c.trim().is_empty()) {
        Some(conv_id) => {
            let rel = save_attachment_bytes(conv_id, attachment_id, &bytes, file_name)
                .context("save composer video local backup")?;
            log::info!(
                "composer video OSS: saved local backup attachment={attachment_id} rel={rel}"
            );
            Some(rel)
        }
        None => None,
    };

    let mut result = upload_composer_video_bytes(
        config,
        attachment_id,
        &bytes,
        file_name,
        content_type,
        compress,
        None,
        READ_END,
        on_progress,
    )
    .await?;
    result.storage_rel_path = storage_rel_path;
    Ok(result)
}

/// Upload bytes via OSS PutObject, return a V4 presigned GET URL for DashScope `video_url`.
pub async fn upload_temp_video(
    config: &MediaOssConfig,
    bytes: &[u8],
    file_name: &str,
    content_type: &str,
) -> Result<OssTempVideo> {
    let resolved = resolve_media_oss_config(config).context("OSS not configured")?;
    if bytes.is_empty() {
        anyhow::bail!("cannot upload empty video to OSS");
    }
    let client = build_client(&resolved)?;
    let object_key = temp_object_key(&resolved.key_prefix, file_name);
    let mime = content_type.trim();
    let mime = if mime.is_empty() { "video/mp4" } else { mime };

    log::info!(
        "media OSS PutObject: bucket={} key={} bytes={} content-type={mime}",
        resolved.bucket,
        object_key,
        bytes.len()
    );

    let options = PutObjectOptions::default().content_type(mime);
    client
        .put_object(&object_key, Bytes::copy_from_slice(bytes), Some(options))
        .await
        .context("OSS PutObject")?;

    let auth_opts = QueryAuthOptions::builder()
        .x_oss_expires(resolved.presign_expires_sec)
        .build();
    let read_url = client
        .presign_get_object(&object_key, true, GetObjectParams::new(), None, auth_opts)
        .await
        .context("OSS presign GET URL")?;

    if !read_url.starts_with("https://") {
        anyhow::bail!("OSS presigned URL must be HTTPS for DashScope video_url");
    }

    log::info!("media OSS: presigned GET ready for {object_key}");
    Ok(OssTempVideo {
        object_key,
        read_url,
    })
}

/// Best-effort delete after understanding (when `deleteAfterUse` is true).
pub async fn delete_temp_object(config: &MediaOssConfig, object_key: &str) -> Result<()> {
    let resolved = resolve_media_oss_config(config).context("OSS not configured")?;
    let client = build_client(&resolved)?;
    client
        .delete_object(object_key, None)
        .await
        .with_context(|| format!("OSS DeleteObject {object_key}"))?;
    log::info!("media OSS: deleted temp object {object_key}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_requires_bucket_region_and_keys() {
        let mut cfg = MediaOssConfig {
            enabled: true,
            bucket: "my-bucket".into(),
            region: "cn-hangzhou".into(),
            access_key_id: "id".into(),
            access_key_secret: "secret".into(),
            ..Default::default()
        };
        assert!(resolve_media_oss_config(&cfg).is_some());
        cfg.access_key_secret.clear();
        assert!(resolve_media_oss_config(&cfg).is_none());
    }

    #[test]
    fn attachment_object_key_includes_yyyymm_month() {
        let month = oss_month_segment();
        assert_eq!(month.len(), 6);
        assert!(month.chars().all(|c| c.is_ascii_digit()));
        let key = attachment_object_key("pointer-media-attachments/", "att-1", "demo.mp4");
        assert!(key.starts_with("pointer-media-attachments/"));
        assert!(key.contains(&format!("/{month}/att-1/demo.mp4")));
    }

    #[test]
    fn temp_object_key_sanitizes_file_name() {
        let key = temp_object_key("pointer-media-temp/", "../../evil clip.mp4");
        assert!(key.starts_with("pointer-media-temp/"));
        let month = oss_month_segment();
        assert!(key.contains(&format!("/{month}/")));
        assert!(key.ends_with("evil_clip.mp4"));
        assert!(!key.contains(".."));
    }

    #[test]
    fn sanitize_object_name_preserves_chinese() {
        assert_eq!(
            sanitize_object_name("像素蛋糕完整示例-0513.mp4"),
            "像素蛋糕完整示例-0513.mp4"
        );
    }
}
