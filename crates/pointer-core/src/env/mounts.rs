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
//! 客户端传入的宿主项目目录一律不是挂载源。会话只改变 `/data` 内的 active cwd。

use std::path::{Path, PathBuf};

use super::pathmap::Mount;
use crate::user_key::{data_volume_name, home_volume_name};

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
    /// 用户级 home/data + 只读 skills。
    pub fn for_session(
        sandboxes_base: &Path,
        session_user_id: &str,
        conversation_id: &str,
        skill_roots: &[SkillRoot],
    ) -> Vec<Mount> {
        Self::for_session_with_plugins(
            sandboxes_base,
            session_user_id,
            conversation_id,
            skill_roots,
            None,
            None,
        )
    }

    /// Same as [`Self::for_session`], plus optional read-only `/plugins`.
    ///
    /// `workspace_root` is ignored (legacy client host project path).
    pub fn for_session_with_plugins(
        sandboxes_base: &Path,
        session_user_id: &str,
        conversation_id: &str,
        skill_roots: &[SkillRoot],
        plugin_root: Option<&Path>,
        workspace_root: Option<&Path>,
    ) -> Vec<Mount> {
        if let Some(project) = workspace_root.filter(|dir| dir.is_dir()) {
            log::warn!(
                "mounts: ignoring client host project mount source host={} conversation={}",
                project.display(),
                conversation_id
            );
        }

        let (home_name, data_name) = volume_names_for(session_user_id, conversation_id);
        let home_host = sandboxes_base.join(home_name);
        let data_host = sandboxes_base.join(data_name);
        ensure_data_layout(&data_host);

        let mut mounts = vec![
            Mount::new(home_host, GUEST_HOME, false),
            Mount::new(data_host, GUEST_DATA, false),
        ];

        let mut used: Vec<String> = Vec::new();
        for root in skill_roots {
            let base_name = sanitize_skill_name(&root.name);
            let mut name = base_name.clone();
            let mut n = 2;
            while used.iter().any(|u| u == &name) {
                name = format!("{base_name}-{n}");
                n += 1;
            }
            used.push(name.clone());
            mounts.push(Mount::new(
                root.path.clone(),
                format!("{GUEST_SKILLS}/{name}"),
                true,
            ));
        }

        if let Some(root) = plugin_root {
            if root.is_dir() {
                mounts.push(Mount::new(root.to_path_buf(), GUEST_PLUGINS, true));
            }
        }
        mounts
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
        (
            format!("anon-{conv}-home"),
            format!("anon-{conv}-data"),
        )
    } else {
        (
            home_volume_name(session_user_id),
            data_volume_name(session_user_id),
        )
    }
}

/// Create the fixed `/data` layout on the host backing directory.
pub fn ensure_data_layout(data_host: &Path) {
    for rel in DATA_LAYOUT_DIRS {
        let path = data_host.join(rel);
        if let Err(error) = std::fs::create_dir_all(&path) {
            log::warn!(
                "mounts: failed to create data layout dir {}: {error:#}",
                path.display()
            );
        }
    }
}

fn sanitize_skill_name(raw: &str) -> String {
    let cleaned = crate::storage::sanitize_storage_dir_segment(raw.trim());
    if cleaned.is_empty() {
        "skill".to_string()
    } else {
        cleaned
    }
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
    fn session_plan_has_home_data_and_skills() {
        let dir = tempfile::tempdir().unwrap();
        let mounts = MountPlan::for_session(dir.path(), "alice", "c-1", &roots());
        assert_eq!(mounts.len(), 4);

        assert_eq!(mounts[0].guest, PathBuf::from(GUEST_HOME));
        assert!(!mounts[0].read_only);
        assert_eq!(
            mounts[0].host,
            dir.path().join(home_volume_name("alice"))
        );

        assert_eq!(mounts[1].guest, PathBuf::from(GUEST_DATA));
        assert!(!mounts[1].read_only);
        assert_eq!(mounts[1].host, dir.path().join(data_volume_name("alice")));
        assert!(mounts[1].host.join("workspaces").is_dir());

        assert_eq!(mounts[2].guest, PathBuf::from("/skills/user"));
        assert!(mounts[2].read_only);
        assert_eq!(mounts[3].guest, PathBuf::from("/skills/bundled"));
    }

    #[test]
    fn authenticated_user_shares_data_across_conversations() {
        let dir = tempfile::tempdir().unwrap();
        let a = MountPlan::for_session(dir.path(), "alice", "c-1", &[]);
        let b = MountPlan::for_session(dir.path(), "alice", "c-2", &[]);
        assert_eq!(a[0].host, b[0].host, "home 跨会话共享");
        assert_eq!(a[1].host, b[1].host, "data 跨会话共享");
        assert_eq!(user_key("alice"), user_key("alice"));
    }

    #[test]
    fn anonymous_sessions_get_isolated_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let a = MountPlan::for_session(dir.path(), "", "c-1", &[]);
        let b = MountPlan::for_session(dir.path(), "", "c-2", &[]);
        assert_ne!(a[0].host, b[0].host);
        assert_ne!(a[1].host, b[1].host);
        assert!(a[0].host.to_string_lossy().contains("anon-"));
    }

    #[test]
    fn different_users_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let a = MountPlan::for_session(dir.path(), "alice", "c-1", &[]);
        let b = MountPlan::for_session(dir.path(), "bob", "c-1", &[]);
        assert_ne!(a[0].host, b[0].host);
        assert_ne!(a[1].host, b[1].host);
    }

    #[test]
    fn path_map_translates_data_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mounts = MountPlan::for_session(dir.path(), "alice", "c-1", &roots());
        let map = PathMap::new(mounts);
        let guest = PathBuf::from("/data/workspaces/demo/a.txt");
        let host = map.to_host(&guest).expect("to_host");
        assert_eq!(
            host,
            dir.path()
                .join(data_volume_name("alice"))
                .join("workspaces/demo/a.txt")
        );
        assert_eq!(map.to_agent(&host).expect("to_agent"), guest);
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
        );
        assert!(mounts.iter().all(|m| m.guest != Path::new(GUEST_WORKSPACE)));
        assert!(mounts.iter().any(|m| m.guest == Path::new(GUEST_DATA)));
        assert!(!mounts.iter().any(|m| m.host == project.path()));
    }

    #[test]
    fn plugin_root_is_mounted_read_only() {
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
        );
        let plugin = mounts
            .iter()
            .find(|m| m.guest == PathBuf::from(GUEST_PLUGINS))
            .expect("plugins mount");
        assert!(plugin.read_only);
    }

    #[test]
    fn volume_names_are_envd_safe() {
        let (home, data) = volume_names_for("alice@x.com", "c-1");
        assert!(crate::user_key::is_volume_safe_name(&home));
        assert!(crate::user_key::is_volume_safe_name(&data));
        assert!(!home.contains('@'));
    }
}
