//! Stateless HMAC-signed public download URLs for IM large-file delivery.
//!
//! Token format: `{base64url(payload)}.{base64url(hmac-sha256)}`
//! Payload JSON: `{ "rel": "...", "exp": unix_secs, "name": "optional" }`
//!
//! No login cookie required. Links expire after [`DEFAULT_TTL_SECS`] (or caller TTL).

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
#[cfg(test)]
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::local_secret;
use crate::storage::app_data_dir;

use super::access::path_has_traversal;
use super::store::{
    app_data_media_rel_from_abs, is_app_data_subtree_rel, media_abs_path_unscoped,
    path_is_under_app_data, CONVERSATION_MEDIA_DIR,
};

type HmacSha256 = Hmac<Sha256>;

const ENV_SECRET: &str = "POINTER_MEDIA_DOWNLOAD_SECRET";
const ENV_PUBLIC_URL: &str = "POINTER_SERVER_PUBLIC_URL";
const ENV_TTL: &str = "POINTER_MEDIA_DOWNLOAD_TTL_SECS";

/// Default link lifetime: 7 days.
pub const DEFAULT_TTL_SECS: u64 = 7 * 24 * 3600;
/// Hard ceiling for TTL (Aliyun-style week max; keeps tokens from living forever).
pub const MAX_TTL_SECS: u64 = 7 * 24 * 3600;
/// Soft cap on downloadable file size via public URL (protects server bandwidth).
pub const PUBLIC_DOWNLOAD_MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenPayload {
    /// Path relative to `{app_data}/` (may include `conversation-media/` or `generated-media/`).
    rel: String,
    exp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

fn signing_secret() -> Result<[u8; 32]> {
    if let Ok(raw) = std::env::var(ENV_SECRET) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("{ENV_SECRET} is set but empty");
        }
        let mut out = [0u8; 32];
        let digest = Sha256::digest(trimmed.as_bytes());
        out.copy_from_slice(&digest);
        return Ok(out);
    }
    local_secret::derive_media_download_key()
}

/// Public base URL for IM download links (`POINTER_SERVER_PUBLIC_URL`).
pub fn public_download_base_url() -> Option<String> {
    std::env::var(ENV_PUBLIC_URL)
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}

pub fn configured_ttl_secs() -> u64 {
    std::env::var(ENV_TTL)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_TTL_SECS)
        .clamp(60, MAX_TTL_SECS)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sign with an explicit key — no env, so tests never touch process state.
fn sign_payload_with(key: &[u8; 32], payload_b64: &str) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(key).context("hmac key")?;
    mac.update(payload_b64.as_bytes());
    let tag = mac.finalize().into_bytes();
    Ok(URL_SAFE_NO_PAD.encode(tag))
}

fn verify_sig_with(key: &[u8; 32], payload_b64: &str, sig_b64: &str) -> Result<()> {
    let expected = sign_payload_with(key, payload_b64)?;
    let a = expected.as_bytes();
    let b = sig_b64.as_bytes();
    if a.len() != b.len() {
        bail!("invalid download token signature");
    }
    use subtle::ConstantTimeEq;
    if !bool::from(a.ct_eq(b)) {
        bail!("invalid download token signature");
    }
    Ok(())
}

/// Encode + sign a payload with an explicit key (no env, no clock).
fn encode_token(key: &[u8; 32], payload: &TokenPayload) -> Result<String> {
    let json = serde_json::to_vec(payload).context("serialize download token")?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(json);
    let sig = sign_payload_with(key, &payload_b64)?;
    Ok(format!("{payload_b64}.{sig}"))
}

/// Verify signature + expiry and return the payload (no env, no clock read).
fn decode_token(key: &[u8; 32], token: &str, now: u64) -> Result<TokenPayload> {
    let trimmed = token.trim();
    let (payload_b64, sig_b64) = trimmed
        .rsplit_once('.')
        .ok_or_else(|| anyhow::anyhow!("malformed download token"))?;
    if payload_b64.is_empty() || sig_b64.is_empty() {
        bail!("malformed download token");
    }
    verify_sig_with(key, payload_b64, sig_b64)?;
    let json = URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .context("decode download token payload")?;
    let payload: TokenPayload =
        serde_json::from_slice(&json).context("parse download token payload")?;
    if payload.exp <= now {
        bail!("download link expired");
    }
    Ok(payload)
}

/// Resolve a token `rel` under an explicit app-data root (no env).
#[cfg(test)]
fn resolve_under_root(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    if rel.is_empty() || path_has_traversal(rel) {
        bail!("invalid token rel path");
    }
    let root = root.canonicalize().context("app data dir")?;
    let path = root.join(rel);
    if !path.is_file() {
        bail!("media file not found: {rel}");
    }
    let canonical = path.canonicalize().context("canonicalize media path")?;
    if !canonical.starts_with(&root) {
        bail!("download path outside app data");
    }
    Ok(canonical)
}

/// Issue a token for a path that is already relative to `root`.
///
/// Same token format as [`issue_download_token`], but the secret, the root and
/// the clock are injected — the shape tests and hosts need.
#[cfg(test)]
pub(crate) fn issue_download_token_at(
    root: &Path,
    key: &[u8; 32],
    rel: &str,
    file_name: Option<&str>,
    ttl_secs: u64,
    now: u64,
) -> Result<String> {
    let abs = resolve_under_root(root, rel)?;
    let meta = std::fs::metadata(&abs).context("stat media file")?;
    if meta.len() > PUBLIC_DOWNLOAD_MAX_BYTES {
        bail!(
            "file too large for public download (max {} MB)",
            PUBLIC_DOWNLOAD_MAX_BYTES / (1024 * 1024)
        );
    }
    let payload = TokenPayload {
        rel: rel.trim().trim_start_matches('/').to_string(),
        exp: now.saturating_add(ttl_secs.clamp(60, MAX_TTL_SECS)),
        name: file_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    };
    encode_token(key, &payload)
}

/// Verify a token against an explicit root + key + clock (no env).
#[cfg(test)]
pub(crate) fn verify_download_at(
    root: &Path,
    key: &[u8; 32],
    token: &str,
    now: u64,
) -> Result<VerifiedPublicDownload> {
    let payload = decode_token(key, token, now)?;
    let path = resolve_under_root(root, &payload.rel)?;
    let meta = std::fs::metadata(&path).context("stat media file")?;
    if meta.len() > PUBLIC_DOWNLOAD_MAX_BYTES {
        bail!("file too large");
    }
    let file_name = payload
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment")
                .to_string()
        });
    let mime_type = mime_guess_from_name(&file_name);
    Ok(VerifiedPublicDownload {
        path,
        file_name,
        mime_type,
    })
}

/// Normalize an absolute or storage-rel path into an app-data-relative path for the token.
pub fn normalize_app_data_rel(path_or_rel: &str) -> Result<String> {
    let trimmed = path_or_rel.trim().trim_start_matches('/');
    if trimmed.is_empty() || path_has_traversal(trimmed) {
        bail!("invalid media path for public download");
    }
    if let Some(rel) = app_data_media_rel_from_abs(std::path::Path::new(trimmed)) {
        return Ok(if is_app_data_subtree_rel(&rel) {
            rel
        } else {
            format!("{CONVERSATION_MEDIA_DIR}/{rel}")
        });
    }
    let abs = if std::path::Path::new(trimmed).is_absolute() {
        std::path::PathBuf::from(trimmed)
    } else {
        resolve_token_rel_to_abs(trimmed)?
    };
    if !path_is_under_app_data(&abs) {
        bail!("public download path outside app data");
    }
    let root = app_data_dir()?.canonicalize().context("app data dir")?;
    let canonical = abs.canonicalize().context("canonicalize media path")?;
    let rel = canonical
        .strip_prefix(&root)
        .context("strip app data prefix")?
        .to_string_lossy()
        .replace('\\', "/");
    if rel.is_empty() || path_has_traversal(&rel) {
        bail!("invalid media rel after normalize");
    }
    Ok(rel)
}

fn resolve_token_rel_to_abs(rel: &str) -> Result<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    if rel.is_empty() || path_has_traversal(rel) {
        bail!("invalid token rel path");
    }
    if rel.starts_with("v2/") {
        return media_abs_path_unscoped(rel);
    }
    if is_app_data_subtree_rel(rel) || rel.starts_with(&format!("{CONVERSATION_MEDIA_DIR}/")) {
        let path = app_data_dir()?.join(rel);
        if path.is_file() {
            return Ok(path);
        }
        bail!("media file not found: {rel}");
    }
    // Legacy conversation-media storage rel without prefix.
    media_abs_path_unscoped(rel)
}

/// Issue a signed token for an on-disk file under app data.
pub fn issue_download_token(
    path_or_rel: &str,
    file_name: Option<&str>,
    ttl_secs: u64,
) -> Result<String> {
    let rel = normalize_app_data_rel(path_or_rel)?;
    let abs = resolve_token_rel_to_abs(&rel)?;
    if !abs.is_file() {
        bail!("media file not found for public download");
    }
    let meta = std::fs::metadata(&abs).context("stat media file")?;
    if meta.len() > PUBLIC_DOWNLOAD_MAX_BYTES {
        bail!(
            "file too large for public download (max {} MB)",
            PUBLIC_DOWNLOAD_MAX_BYTES / (1024 * 1024)
        );
    }
    let ttl = ttl_secs.clamp(60, MAX_TTL_SECS);
    let payload = TokenPayload {
        rel,
        exp: now_unix().saturating_add(ttl),
        name: file_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    };
    encode_token(&signing_secret()?, &payload)
}

/// Build a full public download URL when `POINTER_SERVER_PUBLIC_URL` is set.
pub fn issue_download_url(
    path_or_rel: &str,
    file_name: Option<&str>,
    ttl_secs: u64,
) -> Result<String> {
    let base = public_download_base_url().ok_or_else(|| {
        anyhow::anyhow!("{ENV_PUBLIC_URL} not configured; cannot issue public download link for IM")
    })?;
    let token = issue_download_token(path_or_rel, file_name, ttl_secs)?;
    Ok(format!(
        "{base}/api/media/public-download?token={}",
        urlencoding::encode(&token)
    ))
}

#[derive(Debug, Clone)]
pub struct VerifiedPublicDownload {
    pub path: PathBuf,
    pub file_name: String,
    pub mime_type: String,
}

/// Verify token and resolve the on-disk file (no session login).
pub fn verify_and_resolve_download(token: &str) -> Result<VerifiedPublicDownload> {
    let payload = decode_token(&signing_secret()?, token, now_unix())?;
    if path_has_traversal(&payload.rel) {
        bail!("invalid download path");
    }
    let path = resolve_token_rel_to_abs(&payload.rel)?;
    if !path.is_file() {
        bail!("media file not found");
    }
    if !path_is_under_app_data(&path) {
        bail!("download path outside app data");
    }
    let meta = std::fs::metadata(&path).context("stat media file")?;
    if meta.len() > PUBLIC_DOWNLOAD_MAX_BYTES {
        bail!("file too large");
    }
    let file_name = payload
        .name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("attachment")
                .to_string()
        });
    let mime_type = mime_guess_from_name(&file_name);
    Ok(VerifiedPublicDownload {
        path,
        file_name,
        mime_type,
    })
}

fn mime_guess_from_name(file_name: &str) -> String {
    match std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".into(),
        Some("jpg") | Some("jpeg") => "image/jpeg".into(),
        Some("gif") => "image/gif".into(),
        Some("webp") => "image/webp".into(),
        Some("pdf") => "application/pdf".into(),
        Some("txt") | Some("md") => "text/plain".into(),
        Some("mp4") | Some("m4v") => "video/mp4".into(),
        Some("mp3") => "audio/mpeg".into(),
        Some("wav") => "audio/wav".into(),
        Some("zip") => "application/zip".into(),
        _ => "application/octet-stream".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Fixed key + tempdir root: these tests never touch process env, so a
    /// concurrent test that rewrites `HOME` cannot make them flaky.
    const TEST_KEY: [u8; 32] = [7u8; 32];
    const NOW: u64 = 1_700_000_000;

    fn write_under(root: &Path, rel: &str, body: &[u8]) -> PathBuf {
        let file = root.join(rel);
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(&file, body).expect("write");
        file
    }

    #[test]
    fn issue_and_verify_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let rel = "generated-media/_pub_dl_test/a.txt";
        write_under(root, rel, b"hello-public");

        let token = issue_download_token_at(root, &TEST_KEY, rel, Some("a.txt"), 3600, NOW)
            .expect("issue");
        let verified = verify_download_at(root, &TEST_KEY, &token, NOW).expect("verify");
        assert_eq!(verified.file_name, "a.txt");
        assert_eq!(verified.mime_type, "text/plain");
        assert_eq!(fs::read(&verified.path).expect("read"), b"hello-public");

        // A different key must not verify.
        assert!(verify_download_at(root, &[8u8; 32], &token, NOW).is_err());
        // Expiry is checked against the injected clock, not the wall clock.
        assert!(verify_download_at(root, &TEST_KEY, &token, NOW + 3600).is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let rel = "generated-media/_pub_dl_exp/b.txt";
        write_under(root, rel, b"x");

        let payload = TokenPayload {
            rel: rel.to_string(),
            exp: NOW.saturating_sub(10),
            name: Some("b.txt".into()),
        };
        let token = encode_token(&TEST_KEY, &payload).expect("encode");
        let err = decode_token(&TEST_KEY, &token, NOW).expect_err("expired");
        assert!(
            err.to_string().contains("expired"),
            "expected expired error, got {err:#}"
        );
        // Still decodable while it is valid: the rejection is about `exp` only.
        assert!(decode_token(&TEST_KEY, &token, NOW - 3600).is_ok());
    }

    #[test]
    fn traversal_rels_are_rejected_before_touching_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        assert!(issue_download_token_at(root, &TEST_KEY, "../etc/passwd", None, 60, NOW).is_err());
        assert!(verify_download_at(root, &TEST_KEY, "not-a-token", NOW).is_err());
    }

    #[test]
    fn rejects_traversal_rel() {
        assert!(normalize_app_data_rel("../etc/passwd").is_err());
    }
}
