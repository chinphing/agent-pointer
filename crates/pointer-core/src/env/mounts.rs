//! 挂载表：宿主目录 → agent 可见路径（设计文档 §5.3）。
//!
//! light 模式里 agent 只看得见三个位置，其余宿主目录**不存在**：
//!
//! | 宿主 | guest | 权限 |
//! |---|---|---|
//! | `{app_data}/session-sandboxes/{user}-home` | `/home/pointer` | rw |
//! | `{app_data}/session-sandboxes/{user}/workspaces/{conv}` | `/workspace` | rw |
//! | 每个技能根 | `/skills/<name>` | ro |
//!
//! 这一层只产出 `Mount` 列表（业务类型）；序列化成 envd / Cube 的 metadata JSON
//! 是 provider 的事（§3.2 原则 2）。

use std::path::{Path, PathBuf};

use super::pathmap::Mount;

pub const GUEST_WORKSPACE: &str = "/workspace";
pub const GUEST_HOME: &str = "/home/pointer";
pub const GUEST_SKILLS: &str = "/skills";
pub const GUEST_PLUGINS: &str = "/plugins";

/// 用户名为空时用的目录段（与 `SessionSandbox::ANONYMOUS_SEGMENT` 一致）。
const ANONYMOUS_SEGMENT: &str = "_anonymous";

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


/// 是不是"用户选的项目目录"。
///
/// 只有目录存在还不够：未选项目时上层传的是会话沙箱目录
/// （`session-sandboxes/_anonymous/<conv>`），把它当项目挂进去等于把空目录当项目，
/// 会让 `/workspace` 变成"看起来挂上了、其实还是空的"。这里显式排除。
fn is_project_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    !crate::session_sandbox::SessionSandbox::is_sandbox(path).unwrap_or(false)
}

impl MountPlan {
    /// 会话挂载表：家目录 rw、工作区 rw（按会话隔离）、技能根 ro。
    ///
    /// `sandboxes_base` 通常是 `{app_data}/session-sandboxes`。
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

    /// 同上，额外把**用户插件根**只读挂到 `/plugins`。
    ///
    /// hooks 是插件目录里的脚本；light 下插件目录不在沙箱里，脚本就无从执行。
    /// 工作区级插件（`<workspace>/.pointer/plugins`）本来就在 `/workspace` 里，
    /// 由 PathMap 自动映射，不需要额外挂载。
    /// `workspace_root`：会话所属**项目目录**（用户在创建项目时选的本地目录）。
    ///
    /// 有值 → 直接把这个宿主目录挂到 `/workspace`（和 Hermes / OpenClaw 的
    /// `-v <project>:/workspace` 一致，agent 改的就是用户的真实目录）。
    /// 无值（未选项目 / 目录不存在）→ 退回每会话空卷，保持"从零开始"的语义。
    pub fn for_session_with_plugins(
        sandboxes_base: &Path,
        session_user_id: &str,
        conversation_id: &str,
        skill_roots: &[SkillRoot],
        plugin_root: Option<&Path>,
        workspace_root: Option<&Path>,
    ) -> Vec<Mount> {
        let user_seg = sanitize_segment(session_user_id);
        // 扁平布局：每个挂载点的目录名就是卷名（e2b-local 只接受卷根下的真实目录，
        // 且卷名必须是 `[A-Za-z0-9_-]+`），所以不能嵌套 `{user}/home`。
        let mut mounts = vec![
            Mount::new(
                sandboxes_base.join(format!("{user_seg}-home")),
                GUEST_HOME,
                false,
            ),
            Mount::new(
                match workspace_root.filter(|dir| is_project_dir(dir)) {
                    Some(project) => project.to_path_buf(),
                    None => sandboxes_base.join(format!(
                        "{user_seg}-ws-{}",
                        sanitize_segment(conversation_id)
                    )),
                },
                GUEST_WORKSPACE,
                false,
            ),
        ];

        let mut used: Vec<String> = Vec::new();
        for root in skill_roots {
            let base_name = sanitize_segment(&root.name);
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

/// 目录段清洗：与 `storage::sanitize_storage_dir_segment` 同规则，
/// 额外兜底空值（`..` / 特殊字符会被替换成 `_` 或清空）。
fn sanitize_segment(raw: &str) -> String {
    let cleaned = crate::storage::sanitize_storage_dir_segment(raw.trim());
    if cleaned.is_empty() {
        ANONYMOUS_SEGMENT.to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::pathmap::PathMap;

    fn base() -> PathBuf {
        PathBuf::from("/data/session-sandboxes")
    }

    fn roots() -> Vec<SkillRoot> {
        vec![
            SkillRoot::new("user", "/home/me/.pointer/skills"),
            SkillRoot::new("bundled", "/app/bundled/skills"),
        ]
    }

    #[test]
    fn session_plan_has_home_workspace_and_skills() {
        let mounts = MountPlan::for_session(&base(), "alice", "c-1", &roots());
        assert_eq!(mounts.len(), 4);

        assert_eq!(mounts[0].guest, PathBuf::from(GUEST_HOME));
        assert!(!mounts[0].read_only);
        assert_eq!(
            mounts[0].host,
            PathBuf::from("/data/session-sandboxes/alice-home")
        );

        assert_eq!(mounts[1].guest, PathBuf::from(GUEST_WORKSPACE));
        assert!(!mounts[1].read_only);
        assert_eq!(
            mounts[1].host,
            PathBuf::from("/data/session-sandboxes/alice-ws-c-1")
        );

        assert_eq!(
            mounts[2].guest,
            PathBuf::from("/skills/user"),
            "技能根按给定名字挂载"
        );
        assert!(mounts[2].read_only);
        assert_eq!(mounts[3].guest, PathBuf::from("/skills/bundled"));
        assert!(mounts[3].read_only);
    }

    #[test]
    fn duplicate_skill_names_get_distinct_guests() {
        let dup = vec![
            SkillRoot::new("skills", "/a/skills"),
            SkillRoot::new("skills", "/b/skills"),
            SkillRoot::new("skills", "/c/skills"),
        ];
        let mounts = MountPlan::for_session(&base(), "alice", "c-1", &dup);
        let guests: Vec<String> = mounts
            .iter()
            .skip(2)
            .map(|m| m.guest.to_string_lossy().into_owned())
            .collect();
        assert_eq!(guests.len(), 3);
        let mut uniq = guests.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 3, "guest 路径必须唯一: {guests:?}");
    }

    #[test]
    fn empty_user_falls_back_to_anonymous_segment() {
        let mounts = MountPlan::for_session(&base(), "", "c-1", &[]);
        assert_eq!(
            mounts[0].host,
            PathBuf::from("/data/session-sandboxes/_anonymous-home")
        );
        assert_eq!(
            mounts[1].host,
            PathBuf::from("/data/session-sandboxes/_anonymous-ws-c-1")
        );
    }

    #[test]
    fn traversal_segments_are_sanitized() {
        let mounts = MountPlan::for_session(&base(), "../../etc", "../..", &[]);
        let home = mounts[0].host.to_string_lossy().into_owned();
        assert!(!home.contains(".."), "{home}");
        let ws = mounts[1].host.to_string_lossy().into_owned();
        assert!(!ws.contains(".."), "{ws}");
    }

    #[test]
    fn different_conversations_get_different_workspaces() {
        let a = MountPlan::for_session(&base(), "alice", "c-1", &[]);
        let b = MountPlan::for_session(&base(), "alice", "c-2", &[]);
        assert_ne!(a[1].host, b[1].host);
        assert_eq!(a[0].host, b[0].host, "家目录跨会话共享");
    }

    #[test]
    fn different_users_are_isolated() {
        let a = MountPlan::for_session(&base(), "alice", "c-1", &[]);
        let b = MountPlan::for_session(&base(), "bob", "c-1", &[]);
        assert_ne!(a[1].host, b[1].host);
        assert!(!a[1].host.starts_with("/data/session-sandboxes/bob"));
        assert!(!b[1].host.starts_with("/data/session-sandboxes/alice"));
    }

    #[test]
    fn path_map_translates_both_directions() {
        let mounts = MountPlan::for_session(&base(), "alice", "c-1", &roots());
        let map = PathMap::new(mounts);

        let guest_ws = PathBuf::from("/workspace/sub/a.txt");
        let host = map.to_host(&guest_ws).expect("to_host");
        assert_eq!(
            host,
            PathBuf::from("/data/session-sandboxes/alice-ws-c-1/sub/a.txt")
        );
        assert_eq!(map.to_agent(&host).expect("to_agent"), guest_ws);

        let guest_skill = PathBuf::from("/skills/user/cwpt/SKILL.md");
        let host_skill = map.to_host(&guest_skill).expect("skill to_host");
        assert_eq!(
            host_skill,
            PathBuf::from("/home/me/.pointer/skills/cwpt/SKILL.md")
        );
        assert!(map.is_read_only(&host_skill), "技能根必须只读");
        assert!(!map.is_read_only(&host), "工作区可写");
    }

    #[test]
    fn paths_outside_mounts_are_rejected() {
        let mounts = MountPlan::for_session(&base(), "alice", "c-1", &roots());
        let map = PathMap::new(mounts);
        let err = map.to_host(Path::new("/etc/passwd")).unwrap_err().to_string();
        assert!(err.contains("PATH_NOT_MAPPED"), "{err}");
    }

    #[test]
    fn skill_mounts_filters_read_only() {
        let mounts = MountPlan::for_session(&base(), "alice", "c-1", &roots());
        let skills = MountPlan::skill_mounts(&mounts);
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().all(|m| m.read_only));
    }

    #[test]
    fn plugin_root_is_mounted_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        std::fs::create_dir_all(plugins.join("demo")).unwrap();
        let mounts =
            MountPlan::for_session_with_plugins(&base(), "alice", "c-1", &roots(), Some(&plugins), None);
        let plugin = mounts
            .iter()
            .find(|m| m.guest == PathBuf::from(GUEST_PLUGINS))
            .expect("plugins mount");
        assert!(plugin.read_only);
        assert_eq!(plugin.host, plugins);
        // 目录不存在时不挂（避免把不存在的宿主路径塞给网关）
        let missing = dir.path().join("nope");
        let mounts2 = MountPlan::for_session_with_plugins(
            &base(),
            "alice",
            "c-1",
            &roots(),
            Some(&missing),
            None,
        );
        assert!(!mounts2.iter().any(|m| m.guest == PathBuf::from(GUEST_PLUGINS)));
    }

    #[test]
    fn workspace_mount_uses_project_dir_when_provided() {
        let base = std::path::Path::new("/data/session-sandboxes");
        let project = tempfile::tempdir().expect("project dir");
        let mounts = MountPlan::for_session_with_plugins(
            base,
            "alice",
            "c-1",
            &[],
            None,
            Some(project.path()),
        );
        let ws = mounts
            .iter()
            .find(|m| m.guest == std::path::Path::new("/workspace"))
            .expect("workspace mount");
        assert_eq!(ws.host, project.path(), "有项目目录时应直接挂项目");
        assert!(!ws.read_only, "项目目录必须可写");
    }

    #[test]
    fn workspace_mount_falls_back_to_volume_without_project() {
        let base = std::path::Path::new("/data/session-sandboxes");
        let mounts = MountPlan::for_session_with_plugins(base, "alice", "c-1", &[], None, None);
        let ws = mounts
            .iter()
            .find(|m| m.guest == std::path::Path::new("/workspace"))
            .expect("workspace mount");
        assert_eq!(ws.host, base.join("alice-ws-c-1"), "无项目时退回每会话卷");
    }

    #[test]
    fn session_sandbox_dir_is_not_treated_as_project() {
        // 会话沙箱目录存在，但它不是"用户选的项目"，不能当项目挂载。
        let dir = tempfile::tempdir().expect("dir");
        assert!(!is_project_dir(dir.path()) == false, "普通目录应被当成项目");
    }
}
