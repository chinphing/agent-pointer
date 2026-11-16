//! 宿主 ↔ 环境路径翻译（设计文档 §2.4 / §3.2）。
//!
//! 两条规则：
//!
//! 1. **先 realpath 再匹配前缀**：符号链接解析后仍必须落在某个挂载点内。
//! 2. **未命中一律拒绝**（`PATH_NOT_MAPPED`），不做「猜一个最像的」。
//!
//! local 档位用 [`PathMap::identity`]：`to_host` / `to_agent` 都是恒等，
//! 未命中时直接放行（宿主就是环境）。light / container 用 [`PathMap::new`]，
//! 未命中即报错。

use anyhow::{anyhow, Result};
use std::path::{Component, Path, PathBuf};

/// 业务层描述挂载意图的抽象类型。
///
/// **禁止**在业务层拼 `metadata["host-mount"]` JSON —— provider 负责把它翻译成
/// Cube Volume / Host Mount 或 Docker `-v`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountSpec {
    /// 用户可写数据（宿主 `{app_data}/session-sandboxes/{user}`）→ guest `/home/u`。
    UserData { tenant: String },
    /// 只读共享目录（宿主 `/data/skills-ro`）→ guest `/skills`。
    ReadOnlyShared { name: String },
}

impl MountSpec {
    /// 环境内的挂载点。
    pub fn guest_target(&self) -> PathBuf {
        match self {
            Self::UserData { .. } => PathBuf::from("/home/u"),
            Self::ReadOnlyShared { .. } => PathBuf::from("/skills"),
        }
    }

    pub fn read_only(&self) -> bool {
        matches!(self, Self::ReadOnlyShared { .. })
    }

    /// 日志 / 审计用的稳定标识（不含宿主路径）。
    pub fn logical_name(&self) -> String {
        match self {
            Self::UserData { tenant } => format!("user-data:{tenant}"),
            Self::ReadOnlyShared { name } => format!("shared:{name}"),
        }
    }
}

/// 一条已确定的挂载。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub host: PathBuf,
    pub guest: PathBuf,
    pub read_only: bool,
}

impl Mount {
    pub fn new(host: impl Into<PathBuf>, guest: impl Into<PathBuf>, read_only: bool) -> Self {
        Self {
            host: normalize_lexical(&host.into()),
            guest: normalize_lexical(&guest.into()),
            read_only,
        }
    }
}

/// 路径翻译表。挂载点数量在会话生命周期内固定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    mounts: Vec<Mount>,
    /// 未命中时按恒等放行（local：宿主就是环境）。
    host_passthrough: bool,
}

impl PathMap {
    /// local：恒等映射，任何路径都放行。
    pub fn identity() -> Self {
        Self {
            mounts: Vec::new(),
            host_passthrough: true,
        }
    }

    /// 带挂载表的映射，未命中即拒绝。
    pub fn new(mounts: Vec<Mount>) -> Self {
        Self {
            mounts,
            host_passthrough: false,
        }
    }

    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    pub fn is_host_passthrough(&self) -> bool {
        self.host_passthrough
    }

    /// 环境内路径 → 宿主路径（工具拿到的路径永远是宿主路径）。
    pub fn to_host(&self, env_path: &Path) -> Result<PathBuf> {
        let candidate = canonicalize_if_exists(env_path);
        if let Some((mount, rest)) = longest_prefix(&self.mounts, &candidate, |m| &m.guest) {
            return Ok(join_rest(&mount.host, rest));
        }
        if self.host_passthrough {
            return Ok(candidate);
        }
        Err(anyhow!(
            "PATH_NOT_MAPPED: 环境路径不在任何挂载点内: {}",
            env_path.display()
        ))
    }

    /// 宿主路径 → 环境内路径（工具**输出**的路径必须走这里，否则模型认知漂移）。
    pub fn to_agent(&self, host_path: &Path) -> Result<PathBuf> {
        let candidate = canonicalize_if_exists(host_path);
        if let Some((mount, rest)) = longest_prefix(&self.mounts, &candidate, |m| &m.host) {
            return Ok(join_rest(&mount.guest, rest));
        }
        if self.host_passthrough {
            return Ok(candidate);
        }
        Err(anyhow!(
            "PATH_NOT_MAPPED: 宿主路径不在任何挂载点内: {}",
            host_path.display()
        ))
    }

    /// 该宿主路径是否落在只读挂载下（宿主侧写入的显式拒绝依据，见设计文档 §5.4.1）。
    pub fn is_read_only(&self, host_path: &Path) -> bool {
        let candidate = canonicalize_if_exists(host_path);
        longest_prefix(&self.mounts, &candidate, |m| &m.host)
            .map(|(mount, _)| mount.read_only)
            .unwrap_or(false)
    }
}

fn longest_prefix<'a>(
    mounts: &'a [Mount],
    candidate: &Path,
    prefix_of: impl Fn(&'a Mount) -> &'a PathBuf,
) -> Option<(&'a Mount, PathBuf)> {
    let mut best: Option<(&Mount, PathBuf)> = None;
    for mount in mounts {
        let prefix = prefix_of(mount);
        if let Ok(rest) = candidate.strip_prefix(prefix) {
            let better = match &best {
                Some((current, _)) => prefix.components().count() > prefix_of(current).components().count(),
                None => true,
            };
            if better {
                best = Some((mount, rest.to_path_buf()));
            }
        }
    }
    best
}

fn join_rest(base: &Path, rest: PathBuf) -> PathBuf {
    if rest.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        normalize_lexical(&base.join(rest))
    }
}

/// 已存在的路径先 `realpath`；不存在时按词法归一（`..` / `.` 不落地）。
fn canonicalize_if_exists(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(p) => p,
        Err(_) => normalize_lexical(path),
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_roundtrips_both_directions() {
        let map = PathMap::identity();
        let p = Path::new("/tmp/ws/src/main.rs");
        assert_eq!(map.to_host(p).unwrap(), p);
        assert_eq!(map.to_agent(p).unwrap(), p);
        assert!(map.is_host_passthrough());
        assert!(!map.is_read_only(p));
    }

    #[test]
    fn translates_mount_table_both_directions() {
        let map = PathMap::new(vec![
            Mount::new("/data/skills-ro", "/skills", true),
            Mount::new("/data/session-sandboxes/u1", "/home/u", false),
        ]);
        assert_eq!(
            map.to_host(Path::new("/skills/cwpt/SKILL.md")).unwrap(),
            PathBuf::from("/data/skills-ro/cwpt/SKILL.md")
        );
        assert_eq!(
            map.to_agent(Path::new("/data/session-sandboxes/u1/work/a.txt"))
                .unwrap(),
            PathBuf::from("/home/u/work/a.txt")
        );
    }

    #[test]
    fn rejects_unmapped_paths_with_marker() {
        let map = PathMap::new(vec![Mount::new("/data/skills-ro", "/skills", true)]);
        let err = map.to_host(Path::new("/etc/passwd")).unwrap_err().to_string();
        assert!(err.contains("PATH_NOT_MAPPED"), "{err}");
        let err = map.to_agent(Path::new("/etc/passwd")).unwrap_err().to_string();
        assert!(err.contains("PATH_NOT_MAPPED"), "{err}");
    }

    #[test]
    fn longest_prefix_wins() {
        let map = PathMap::new(vec![
            Mount::new("/home/u", "/home/u", false),
            Mount::new("/home/u/.cache", "/home/u/cache", true),
        ]);
        assert!(map.is_read_only(Path::new("/home/u/.cache/x")));
        assert!(!map.is_read_only(Path::new("/home/u/notes/x")));
        assert_eq!(
            map.to_agent(Path::new("/home/u/.cache/x")).unwrap(),
            PathBuf::from("/home/u/cache/x")
        );
    }

    #[test]
    fn read_only_mount_is_reported_for_skills() {
        let map = PathMap::new(vec![Mount::new("/data/skills-ro", "/skills", true)]);
        assert!(map.is_read_only(Path::new("/data/skills-ro/any/SKILL.md")));
    }

    #[test]
    fn parent_components_do_not_escape_mount_root() {
        let map = PathMap::new(vec![Mount::new("/data/skills-ro", "/skills", true)]);
        // `/skills/../../etc` 词法归一后是 `/etc`，不在挂载点内 → 拒绝。
        assert!(map.to_host(Path::new("/skills/../../etc")).is_err());
    }

    #[test]
    fn mount_spec_maps_to_guest_target() {
        let user = MountSpec::UserData {
            tenant: "u1".into(),
        };
        assert_eq!(user.guest_target(), PathBuf::from("/home/u"));
        assert!(!user.read_only());
        assert_eq!(user.logical_name(), "user-data:u1");

        let shared = MountSpec::ReadOnlyShared {
            name: "cwpt".into(),
        };
        assert_eq!(shared.guest_target(), PathBuf::from("/skills"));
        assert!(shared.read_only());
        assert_eq!(shared.logical_name(), "shared:cwpt");
    }
}
