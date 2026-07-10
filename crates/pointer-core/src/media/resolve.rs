//! Unified local media path resolution under [`crate::storage::app_data_dir`].
//!
//! Conversation attachment rel paths (`{conv}/{name}_{suffix}`) always resolve via
//! [`super::store::media_abs_path`] — never via process `cwd`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::storage::app_data_dir;

use super::access::{is_user_filesystem_path, normalize_user_path, path_has_traversal};
use super::store::{
    is_app_data_subtree_rel, media_abs_path, media_abs_path_unscoped, CONVERSATION_MEDIA_DIR,
};

/// True for persisted attachment rel paths (not user absolute/`~/` paths).
pub fn is_storage_rel_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_user_filesystem_path(trimmed) {
        return false;
    }
    let rel = trimmed.trim_start_matches('/');
    !rel.is_empty()
        && !path_has_traversal(rel)
        && !is_app_data_subtree_rel(rel)
        && rel.contains('/')
}

/// Resolve a local media path to an existing file or directory.
///
/// Order: user path → storage rel → explicit `./`/`../` workspace rel → `{app_data}/…`.
/// Does **not** fall back to `cwd` for storage rel paths.
pub fn resolve_local_media_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty media path");
    }
    if path_has_traversal(trimmed) {
        anyhow::bail!("media path traversal not allowed: {trimmed}");
    }

    if is_user_filesystem_path(trimmed) {
        let path = normalize_user_path(trimmed)?;
        if path.is_file() || path.is_dir() {
            return Ok(path);
        }
        anyhow::bail!("media file not found: {trimmed}");
    }

    if is_app_data_subtree_rel(trimmed) {
        let rel = trimmed.trim_start_matches('/');
        let path = app_data_dir()?.join(rel);
        if path.is_file() || path.is_dir() {
            return Ok(path);
        }
        anyhow::bail!("media file not found under app data: {}", path.display());
    }

    if is_storage_rel_path(trimmed) {
        let rel = trimmed.trim_start_matches('/');
        let path = match media_abs_path(rel) {
            Ok(p) if p.is_file() || p.is_dir() => p,
            Ok(p) => match media_abs_path_unscoped(rel) {
                Ok(p2) if p2.is_file() || p2.is_dir() => p2,
                _ => p,
            },
            Err(e) if e.to_string().contains("media access denied") => {
                media_abs_path_unscoped(rel).with_context(|| {
                    format!("resolve storage rel path {trimmed} (session access denied, unscoped fallback)")
                })?
            }
            Err(e) => {
                log::warn!(
                    "resolve_local_media_path media_abs_path FAILED: trimmed={trimmed} error: {e:#}"
                );
                return Err(e).with_context(|| format!("resolve storage rel path {trimmed}"));
            }
        };
        if path.is_file() || path.is_dir() {
            return Ok(path);
        }
        log::warn!(
            "resolve_local_media_path file NOT FOUND: trimmed={trimmed} resolved={}",
            path.display()
        );
        anyhow::bail!("media file not found under app data: {}", path.display());
    }

    if trimmed.starts_with("./") || trimmed.starts_with("../") {
        let path = normalize_user_path(trimmed)?;
        if path.is_file() || path.is_dir() {
            return Ok(path);
        }
        anyhow::bail!("media file not found: {trimmed}");
    }

    if let Ok(data_dir) = app_data_dir() {
        let under_data = data_dir.join(trimmed.trim_start_matches('/'));
        if under_data.is_file() || under_data.is_dir() {
            return Ok(under_data);
        }
        let under_media = data_dir
            .join(CONVERSATION_MEDIA_DIR)
            .join(trimmed.trim_start_matches('/'));
        if under_media.is_file() || under_media.is_dir() {
            return Ok(under_media);
        }
    }

    anyhow::bail!("media file not found: {trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn storage_rel_is_not_user_path() {
        assert!(is_storage_rel_path(
            "weixin_default_weixin_dm_o9cq80w5c6zrEn3gvKlFfwKNhdco_im_wechat_s1/abc_video.mp4"
        ));
        assert!(!is_user_filesystem_path(
            "weixin_default_weixin_dm_o9cq80w5c6zrEn3gvKlFfwKNhdco_im_wechat_s1/abc_video.mp4"
        ));
    }

    #[test]
    fn normalize_user_path_does_not_join_cwd_for_storage_rel() {
        let raw = "conv_folder/attachment_id_file.mp4";
        let path = normalize_user_path(raw).expect("normalize");
        assert_eq!(path, Path::new(raw));
        assert!(!path.is_absolute());
    }

    #[test]
    fn resolve_generated_media_rel_under_app_data() {
        use super::super::store::GENERATED_MEDIA_PREFIX;
        use crate::storage::app_data_dir;
        use std::fs;

        let root = app_data_dir().expect("app data");
        let rel = format!("{GENERATED_MEDIA_PREFIX}_resolve_test/conv/a.png");
        let file = root.join(&rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file, b"img").unwrap();

        let path = resolve_local_media_path(&rel).expect("resolve generated media");
        assert_eq!(path, file);

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(root.join("generated-media/_resolve_test"));
    }

    #[test]
    fn generated_media_prefix_is_app_data_subtree_not_storage_rel() {
        use super::super::store::is_app_data_subtree_rel;
        let raw = "generated-media/user/conv/abc.png";
        assert!(is_app_data_subtree_rel(raw));
        assert!(!is_storage_rel_path(raw));
    }

    #[test]
    fn session_sandbox_prefix_is_app_data_subtree_not_storage_rel() {
        use super::super::store::is_app_data_subtree_rel;
        let raw = "session-sandboxes/user/conv/out.png";
        assert!(is_app_data_subtree_rel(raw));
        assert!(!is_storage_rel_path(raw));
    }
}
