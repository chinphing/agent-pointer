//! Resolve optional reference `image` args (local path, `~`, URL) for generation APIs.

use anyhow::{Context, Result};
use base64::Engine;

const MAX_REF_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Turn tool `image` / `imageUrl` into a value cloud APIs accept (HTTP(S) or `data:` URL).
pub fn resolve_reference_image_for_api(raw: &str) -> Result<String> {
    resolve_reference_image_for_api_in_env(raw, None)
}

/// 同 [`resolve_reference_image_for_api`]，本地路径按**会话环境**解析。
///
/// 沙箱档位下参考图可能是工作区路径（`/data/.../ref.png`），宿主上没有这个文件；
/// `env = None`（没有会话环境 / local 档位未缓存）时行为与
/// [`resolve_reference_image_for_api`] 完全一致（宿主读取）。
pub fn resolve_reference_image_for_api_in_env(
    raw: &str,
    env: Option<&dyn crate::env::Environment>,
) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty reference image");
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
    {
        return Ok(trimmed.to_string());
    }

    let path_ref = trimmed
        .strip_prefix("file://")
        .map(str::trim)
        .unwrap_or(trimmed);

    let preview = crate::media::read_media_ref_preview_in_env(path_ref, env)
        .with_context(|| format!("read reference image {path_ref}"))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(preview.data_base64.trim())
        .context("decode reference image base64")?;
    if bytes.len() > MAX_REF_IMAGE_BYTES {
        anyhow::bail!(
            "reference image exceeds {} MB",
            MAX_REF_IMAGE_BYTES / (1024 * 1024)
        );
    }

    let mime = preview.mime_type.trim();
    if !mime.starts_with("image/") {
        anyhow::bail!("reference image must be an image file (got {mime})");
    }

    log::info!(
        "media_generation: resolved reference image path={path_ref} bytes={} mime={mime}",
        bytes.len()
    );

    Ok(format!("data:{mime};base64,{}", preview.data_base64.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_https() {
        let u = "https://example.com/a.png";
        assert_eq!(resolve_reference_image_for_api(u).unwrap(), u);
    }

    #[test]
    fn resolves_tilde_desktop_jpg_if_present() {
        let home = dirs::home_dir().expect("home");
        let path = home.join("Desktop/baby_cover.jpg");
        if !path.is_file() {
            return;
        }
        let raw = "~/Desktop/baby_cover.jpg";
        let out = resolve_reference_image_for_api(raw).expect("resolve");
        assert!(out.starts_with("data:image/"));
        assert!(out.contains(";base64,"));
    }
}
