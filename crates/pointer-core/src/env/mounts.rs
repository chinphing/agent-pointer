//! 挂载表：宿主目录 → agent 可见路径。
//!
//! headless/desktop 里 agent 只看得见这些位置，其余宿主目录不存在：
//!
//! | 宿主 | guest | 权限 |
//! |---|---|---|
//! | `{base}/{user_key}-home` | `/home/pointer` | rw |
//! | `{base}/{user_key}-data` | `/data` | rw |
//! | 每个技能根 | `/skills/<name>` | ro |
//! | 用户插件根 | `/plugins` | ro |
//!
//! 容器里的工作区是 `/data` 或 `/home/pointer` 下用户选中的目录。
//! 没有选中目录时，工作区是 `/data/workspaces`。宿主目录不挂进容器。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::pathmap::Mount;
use crate::user_data_paths::{user_data_host, user_home_host};

/// Deprecated guest alias. New mounts use [`GUEST_DATA`]; keep the constant so
/// older call sites that still mention `/workspace` fail loudly in tests.
pub const GUEST_WORKSPACE: &str = "/workspace";
pub const GUEST_DATA: &str = "/data";
pub const GUEST_HOME: &str = "/home/pointer";
pub const GUEST_SKILLS: &str = "/skills";
pub const GUEST_PLUGINS: &str = "/plugins";
pub const GUEST_WORKSPACES_ROOT: &str = "/data/workspaces";

/// Layout directories created under the host data volume.
pub const DATA_LAYOUT_DIRS: &[&str] = &[
    "workspaces",
    "worktrees",
    "attachments",
    "media/chat",
    "media/generated",
    "captures",
    "memories",
    "turn-baselines",
];

/// 一个技能根：`name` 决定它在沙箱里的挂载点 `/skills/<name>`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRoot {
    pub name: String,
    pub path: PathBuf,
}

impl SkillRoot {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }
}

/// 会话挂载表构建。
pub struct MountPlan;

impl MountPlan {
    /// 用户级 home/data。技能和插件在该用户 home 里，不再单独挂载。
    ///
    /// `users_root` 是 `{系统目录}/users`（或部署指定的用户根）。
    /// `skill_roots` 保留参数以兼容旧调用，不进入挂载表。
    pub fn for_session(
        users_root: &Path,
        session_user_id: &str,
        conversation_id: &str,
        _skill_roots: &[SkillRoot],
    ) -> Result<Vec<Mount>> {
        Self::for_session_with_plugins(
            users_root,
            session_user_id,
            conversation_id,
            &[],
            None,
            None,
        )
    }

    /// Same as [`Self::for_session`]. Plugin and skill roots are not mounted.
    ///
    /// `workspace_root` is ignored (legacy client host project path).
    pub fn for_session_with_plugins(
        users_root: &Path,
        session_user_id: &str,
        conversation_id: &str,
        _skill_roots: &[SkillRoot],
        _plugin_root: Option<&Path>,
        workspace_root: Option<&Path>,
    ) -> Result<Vec<Mount>> {
        if let Some(project) = workspace_root.filter(|dir| dir.is_dir()) {
            log::info!(
                "mounts: ignoring client host project mount source host={} conversation={}",
                project.display(),
                conversation_id
            );
        }

        let home_host = user_home_host(users_root, session_user_id, conversation_id)?;
        let data_host = user_data_host(users_root, session_user_id, conversation_id)?;
        std::fs::create_dir_all(&home_host)
            .with_context(|| format!("create home {}", home_host.display()))?;
        ensure_data_layout(&data_host)?;

        Ok(vec![
            Mount::new(home_host, GUEST_HOME, false),
            Mount::new(data_host, GUEST_DATA, false),
        ])
    }

    /// 只读技能根（`/skills/**`），供凭证检查 / 发布前扫描使用。
    pub fn skill_mounts(mounts: &[Mount]) -> Vec<&Mount> {
        mounts.iter().filter(|m| m.read_only).collect()
    }
}

/// Host volume directory names for this identity.
///
/// Authenticated users share one home/data pair. Anonymous sessions stay
/// conversation-scoped so they do not collide on `anon-data`.
pub fn volume_names_for(session_user_id: &str, conversation_id: &str) -> (String, String) {
    if session_user_id.trim().is_empty() {
        let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
        let conv = if conv.is_empty() {
            "session".to_string()
        } else {
            conv
        };
        (format!("anon-{conv}-home"), format!("anon-{conv}-data"))
    } else {
        (
            crate::user_key::home_volume_name(session_user_id),
            crate::user_key::data_volume_name(session_user_id),
        )
    }
}

/// Create the fixed data layout on the host backing directory.
pub fn ensure_data_layout(data_host: &Path) -> Result<()> {
    for rel in DATA_LAYOUT_DIRS {
        let path = data_host.join(rel);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("create data layout dir {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::pathmap::PathMap;

    fn roots() -> Vec<SkillRoot> {
        vec![
            SkillRoot::new("user", "/home/me/.pointer/skills"),
            SkillRoot::new("bundled", "/app/bundled/skills"),
        ]
    }

    #[test]
    fn session_plan_has_only_home_and_data() {
        let dir = tempfile::tempdir().unwrap();
        let mounts = MountPlan::for_session(dir.path(), "alice", "c-1", &roots()).unwrap();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].guest, PathBuf::from(GUEST_HOME));
        assert!(!mounts[0].read_only);
        assert!(mounts[0].host.ends_with("home"));
        assert_eq!(mounts[1].guest, PathBuf::from(GUEST_DATA));
        assert!(mounts[1].host.join("workspaces").is_dir());
        assert!(mounts[1].host.join("memories").is_dir());
        assert!(mounts.iter().all(|m| !m.read_only));
    }

    #[test]
    fn authenticated_user_shares_data_across_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let a = MountPlan::for_session(dir.path(), "alice", "c-1", &[]).unwrap();
        let b = MountPlan::for_session(dir.path(), "alice", "c-2", &[]).unwrap();
        assert_eq!(a[0].host, b[0].host, "home 跨会话共享");
        assert_eq!(a[1].host, b[1].host, "data 跨会话共享");
    }

    #[test]
    fn anonymous_sessions_get_isolated_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let a = MountPlan::for_session(dir.path(), "", "c-1", &[]).unwrap();
        let b = MountPlan::for_session(dir.path(), "", "c-2", &[]).unwrap();
        assert_ne!(a[0].host, b[0].host);
        assert_ne!(a[1].host, b[1].host);
        assert!(a[0].host.to_string_lossy().contains("anon"));
    }

    #[test]
    fn different_users_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let a = MountPlan::for_session(dir.path(), "alice", "c-1", &[]).unwrap();
        let b = MountPlan::for_session(dir.path(), "bob", "c-1", &[]).unwrap();
        assert_ne!(a[0].host, b[0].host);
        assert_ne!(a[1].host, b[1].host);
    }

    #[test]
    fn path_map_translates_data_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mounts = MountPlan::for_session(dir.path(), "alice", "c-1", &roots()).unwrap();
        let map = PathMap::new(mounts);
        let guest = PathBuf::from("/data/workspaces/demo/a.txt");
        let host = map.to_host(&guest).expect("to_host");
        assert!(
            host.ends_with("workspaces/demo/a.txt") || host.ends_with("workspaces\\demo\\a.txt")
        );
        assert_eq!(map.to_agent(&host).expect("to_agent"), guest);
        assert!(map
            .to_host(Path::new("/root/.local/share/PointerApp/secret"))
            .is_err());
    }

    #[test]
    fn client_host_project_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mounts = MountPlan::for_session_with_plugins(
            dir.path(),
            "alice",
            "c-1",
            &[],
            None,
            Some(project.path()),
        )
        .unwrap();
        assert!(mounts.iter().all(|m| m.guest != Path::new(GUEST_WORKSPACE)));
        assert!(mounts.iter().any(|m| m.guest == Path::new(GUEST_DATA)));
        assert!(!mounts.iter().any(|m| m.host == project.path()));
    }

    #[test]
    fn plugin_root_is_not_mounted() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        std::fs::create_dir_all(plugins.join("demo")).unwrap();
        let mounts = MountPlan::for_session_with_plugins(
            dir.path(),
            "alice",
            "c-1",
            &roots(),
            Some(&plugins),
            None,
        )
        .unwrap();
        assert!(mounts
            .iter()
            .all(|m| m.guest != PathBuf::from(GUEST_PLUGINS)));
        assert_eq!(mounts.len(), 2);
    }

    #[test]
    fn volume_names_are_envd_safe() {
        let (home, data) = volume_names_for("alice@x.com", "c-1");
        assert!(crate::user_key::is_volume_safe_name(&home));
        assert!(crate::user_key::is_volume_safe_name(&data));
        assert!(!home.contains('@'));
    }
}
