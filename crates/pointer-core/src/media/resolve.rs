//! Unified local media path resolution under [`crate::storage::app_data_dir`].
//!
//! Conversation attachment rel paths (`{conv}/{name}_{suffix}`) always resolve via
//! [`super::store::media_abs_path`] — never via process `cwd`.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::storage::app_data_dir;

use super::access::{
    is_user_filesystem_path, normalize_user_path, path_has_traversal, recover_nested_absolute_path,
};
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
            return accept_host_media_path(path);
        }
        if let Some(nested) = recover_nested_absolute_path(&path) {
            log::warn!(
                "resolve_local_media_path: recovered nested absolute path {} from {}",
                nested.display(),
                path.display()
            );
            return accept_host_media_path(nested);
        }
        anyhow::bail!("media file not found: {trimmed}");
    }

    if is_app_data_subtree_rel(trimmed) {
        let rel = trimmed.trim_start_matches('/');
        let path = app_data_dir()?.join(rel);
        if path.is_file() || path.is_dir() {
            return accept_stored_media_path(path);
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
            return accept_stored_media_path(path);
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
            return accept_host_media_path(path);
        }
        anyhow::bail!("media file not found: {trimmed}");
    }

    if let Ok(data_dir) = app_data_dir() {
        let under_data = data_dir.join(trimmed.trim_start_matches('/'));
        if under_data.is_file() || under_data.is_dir() {
            return accept_stored_media_path(under_data);
        }
        let under_media = data_dir
            .join(CONVERSATION_MEDIA_DIR)
            .join(trimmed.trim_start_matches('/'));
        if under_media.is_file() || under_media.is_dir() {
            return accept_stored_media_path(under_media);
        }
    }

    anyhow::bail!("media file not found: {trimmed}")
}

/// 已经按库存相对路径解析过的文件。有会话时仍必须落在该会话的路径表里。
fn accept_stored_media_path(path: PathBuf) -> Result<PathBuf> {
    if let Some(session) = crate::env::current_session() {
        crate::media::media_ref::session_host_file(session.environment().as_ref(), &path)?;
    }
    Ok(path)
}

/// 调用方直接给出的宿主路径。没有会话，或路径不在会话的 home / data 里，就拒绝。
fn accept_host_media_path(path: PathBuf) -> Result<PathBuf> {
    crate::media::media_ref::ensure_inside_user_env(&path)?;
    Ok(path)
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

        // Resolution reads the process-global app data dir, so take the same lock
        // every other test that replaces it uses, and pin a directory of our own.
        let _lock = crate::storage::test_app_data_dir_lock();
        let dir = tempfile::tempdir().expect("temp data dir");
        crate::storage::set_test_app_data_dir(dir.path().to_path_buf());

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

    #[test]
    fn resolve_recovers_absolute_path_joined_under_sandbox() {
        use std::fs;
        let real = std::env::temp_dir().join(format!(
            "pointer-media-nested-{}.json",
            uuid::Uuid::new_v4()
        ));
        fs::write(&real, b"{\"ok\":true}").unwrap();
        let real_unix = real.to_string_lossy().replace('\\', "/");
        let suffix = real_unix.trim_start_matches('/');
        let doubled = format!(
            "/Users/alice/Library/Application Support/PointerApp/session-sandboxes/00000000-0000-0000-0000-000000000000/{suffix}"
        );
        let err = resolve_local_media_path(&doubled)
            .expect_err("nested host path must not open without a session");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("缺少当前用户的执行环境") || msg.contains("PATH_NOT_MAPPED"),
            "{msg}"
        );
        let _ = fs::remove_file(&real);
    }
}
