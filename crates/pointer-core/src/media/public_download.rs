//! Stateless HMAC-signed public download URLs for IM large-file delivery.
//!
//! Token format: `{base64url(payload)}.{base64url(hmac-sha256)}`
//! Payload JSON: `{ "user": "<user_key|anon/conversation>", "root": "home|data", "rel": "...", "exp": unix_secs, "name": "optional" }`
//!
//! Tokens that carry a host absolute path, or the older payload with only `rel`, do not redeem.
//!
//! No login cookie required. Links expire after [`DEFAULT_TTL_SECS`] (or caller TTL).

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
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
    /// Directory name under `{app data}/users`: a user key, or `anon/{conversation}`.
    #[serde(default)]
    user: String,
    /// `home` or `data`.
    #[serde(default)]
    root: String,
    /// Path relative to that root. Never a host absolute path.
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

/// Redeem `user` + `root` + `rel` under `{app_data}/users`. Host absolute paths fail
/// before the file is opened.
fn resolve_token_payload(app_data: &std::path::Path, payload: &TokenPayload) -> Result<PathBuf> {
    let user = payload.user.trim();
    let root_kind = payload.root.trim();
    if user.is_empty() || root_kind.is_empty() {
        log::warn!("public download: token missing user or root");
        bail!("旧的下载令牌无法兑换");
    }
    if crate::user_data_paths::is_host_absolute(&payload.rel) {
        log::warn!("public download: token contains a host absolute path");
        bail!("下载令牌包含宿主绝对路径");
    }
    if user.contains("..") || user.starts_with('/') || user.contains('\\') {
        bail!("下载令牌用户无效");
    }
    let stored = crate::user_data_paths::UserRelPath::decode(&format!(
        "{root_kind}:{}",
        payload.rel.trim().trim_start_matches('/')
    ))?;
    let users = app_data.join("users");
    let tree = if let Some(conv) = user.strip_prefix("anon/") {
        if conv.is_empty() || conv.contains('/') || conv.contains("..") {
            bail!("下载令牌用户无效");
        }
        users.join("anon").join(conv)
    } else if user.contains('/') {
        bail!("下载令牌用户无效");
    } else {
        users.join(user)
    };
    let home = tree.join("home");
    let data = tree.join("data");
    let root = match stored.root {
        crate::user_data_paths::UserRootKind::Home => &home,
        crate::user_data_paths::UserRootKind::Data => &data,
    };
    let path = stored.join_host(&home, &data);
    if !path.starts_with(root) {
        log::warn!("public download: rel escaped user tree {}", path.display());
        bail!("下载路径不在该用户目录内");
    }
    if !path.is_file() {
        bail!("media file not found");
    }
    let canonical = path.canonicalize().context("canonicalize media path")?;
    let root_canonical = root.canonicalize().context("user root")?;
    if !canonical.starts_with(&root_canonical) {
        log::warn!(
            "public download: canonical path left user tree {}",
            canonical.display()
        );
        bail!("下载路径不在该用户目录内");
    }
    Ok(canonical)
}

/// Issue a token for a file already inside `{app_data}/users/{user}/{root}/{rel}`.
#[cfg(test)]
pub(crate) fn issue_download_token_at(
    app_data: &Path,
    key: &[u8; 32],
    user: &str,
    root: &str,
    rel: &str,
    file_name: Option<&str>,
    ttl_secs: u64,
    now: u64,
) -> Result<String> {
    let payload = TokenPayload {
        user: user.to_string(),
        root: root.to_string(),
        rel: rel.trim().trim_start_matches('/').to_string(),
        exp: now.saturating_add(ttl_secs.clamp(60, MAX_TTL_SECS)),
        name: file_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    };
    let abs = resolve_token_payload(app_data, &payload)?;
    let meta = std::fs::metadata(&abs).context("stat media file")?;
    if meta.len() > PUBLIC_DOWNLOAD_MAX_BYTES {
        bail!(
            "file too large for public download (max {} MB)",
            PUBLIC_DOWNLOAD_MAX_BYTES / (1024 * 1024)
        );
    }
    encode_token(key, &payload)
}

/// Verify a token against an explicit app-data root + key + clock (no env).
#[cfg(test)]
pub(crate) fn verify_download_at(
    app_data: &Path,
    key: &[u8; 32],
    token: &str,
    now: u64,
) -> Result<VerifiedPublicDownload> {
    let payload = decode_token(key, token, now)?;
    let path = resolve_token_payload(app_data, &payload)?;
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

fn locate_host_file(path: &std::path::Path) -> Result<(String, String, String)> {
    let users = crate::user_data_paths::users_root()?
        .canonicalize()
        .context("users root")?;
    let canonical = path.canonicalize().context("canonicalize media path")?;
    let rest = canonical.strip_prefix(&users).map_err(|_| {
        log::warn!(
            "public download: path is outside the user tree {}",
            canonical.display()
        );
        anyhow::anyhow!("公开下载路径不在用户目录内")
    })?;
    let parts: Vec<String> = rest
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(|s| s.to_string()))
        .collect();
    let (user, root_index) = if parts.first().map(String::as_str) == Some("anon") {
        if parts.len() < 4 {
            bail!("公开下载路径不在用户目录内");
        }
        (format!("anon/{}", parts[1]), 2)
    } else if parts.len() >= 3 {
        (parts[0].clone(), 1)
    } else {
        bail!("公开下载路径不在用户目录内");
    };
    let root = match parts[root_index].as_str() {
        "home" => "home",
        "data" => "data",
        _ => bail!("公开下载路径不在用户的 home 或 data 内"),
    };
    let rel = parts[root_index + 1..].join("/");
    let stored = crate::user_data_paths::UserRelPath::decode(&format!("{root}:{rel}"))?;
    Ok((
        user,
        root.to_string(),
        stored.rel.to_string_lossy().replace('\\', "/"),
    ))
}

fn locate_for_issue(path_or_rel: &str) -> Result<(String, String, String, PathBuf)> {
    if crate::user_data_paths::is_host_absolute(path_or_rel) {
        let (user, root, rel) = locate_host_file(std::path::Path::new(path_or_rel))?;
        let path = std::path::PathBuf::from(path_or_rel)
            .canonicalize()
            .context("canonicalize media path")?;
        return Ok((user, root, rel, path));
    }
    if let Ok(stored) = crate::user_data_paths::UserRelPath::decode(path_or_rel) {
        let session = crate::env::current_session().context("公开下载缺少当前用户的执行环境")?;
        let user = if let Some(key) = session.scope().id.strip_prefix("u:") {
            key.to_string()
        } else if let Some(conv) = session.scope().id.strip_prefix("c:") {
            format!(
                "anon/{}",
                crate::storage::sanitize_storage_dir_segment(conv)
            )
        } else {
            bail!("公开下载缺少当前用户");
        };
        let path = stored.join_host(&session.scope().home_host, &session.scope().data_host);
        if !path.is_file() {
            bail!("media file not found for public download");
        }
        let root = match stored.root {
            crate::user_data_paths::UserRootKind::Home => "home",
            crate::user_data_paths::UserRootKind::Data => "data",
        };
        return Ok((
            user,
            root.to_string(),
            stored.rel.to_string_lossy().replace('\\', "/"),
            path,
        ));
    }
    let abs = resolve_token_rel_to_abs(path_or_rel)?;
    let (user, root, rel) = locate_host_file(&abs)?;
    Ok((user, root, rel, abs))
}

/// Issue a signed token for a file inside a user tree.
///
/// The payload is the user directory, the root kind, the relative segment, and
/// the expiry. A host absolute path is not stored.
pub fn issue_download_token(
    path_or_rel: &str,
    file_name: Option<&str>,
    ttl_secs: u64,
) -> Result<String> {
    let (user, root, rel, abs) = locate_for_issue(path_or_rel)?;
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
        user,
        root,
        rel,
        exp: now_unix().saturating_add(ttl),
        name: file_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
    };
    log::info!(
        "public download: issued user={} root={} rel={}",
        payload.user,
        payload.root,
        payload.rel
    );
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
    let path = resolve_token_payload(&app_data_dir()?, &payload)?;
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
        write_under(root, &format!("users/uabc/data/{rel}"), b"hello-public");

        let token = issue_download_token_at(
            root,
            &TEST_KEY,
            "uabc",
            "data",
            rel,
            Some("a.txt"),
            3600,
            NOW,
        )
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
            user: "uabc".into(),
            root: "data".into(),
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
        assert!(issue_download_token_at(
            root,
            &TEST_KEY,
            "uabc",
            "data",
            "../etc/passwd",
            None,
            60,
            NOW
        )
        .is_err());
        assert!(verify_download_at(root, &TEST_KEY, "not-a-token", NOW).is_err());
    }

    #[test]
    fn old_and_absolute_tokens_do_not_open_a_host_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let secret = tmp.path().join("secret.txt");
        fs::write(&secret, b"host-secret").expect("secret");
        let old = TokenPayload {
            user: String::new(),
            root: String::new(),
            rel: secret.display().to_string(),
            exp: NOW + 60,
            name: None,
        };
        let token = encode_token(&TEST_KEY, &old).expect("encode");
        let err = verify_download_at(tmp.path(), &TEST_KEY, &token, NOW).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("旧的下载令牌"), "{msg}");
        assert!(!msg.contains("host-secret"), "{msg}");

        let absolute = TokenPayload {
            user: "uabc".into(),
            root: "data".into(),
            rel: secret.display().to_string(),
            exp: NOW + 60,
            name: None,
        };
        let token = encode_token(&TEST_KEY, &absolute).expect("encode");
        let err = verify_download_at(tmp.path(), &TEST_KEY, &token, NOW).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("宿主绝对路径") || msg.contains("不能作为用户路径"),
            "{msg}"
        );
        assert!(!msg.contains("host-secret"), "{msg}");
    }

    #[test]
    fn rejects_traversal_rel() {
        assert!(normalize_app_data_rel("../etc/passwd").is_err());
    }
}
