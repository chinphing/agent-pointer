//! Per-user home and data directories.
//!
//! The system directory stays `{app data}`. Each Pointer account gets
//! `{app data}/users/{user_key}/home` and `.../data`. Anonymous sessions use
//! `{app data}/users/anon/{conversation}`. Feature code joins the relative
//! segment onto `Environment::home_root` / `data_root` after the session
//! environment exists. These helpers only resolve the host backing tree.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::env::mounts::ensure_data_layout;
use crate::user_key::user_key;

/// Which of the two writable roots a stored path belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRootKind {
    Home,
    Data,
}

/// Root plus a relative segment. This is the shape stored in shared databases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRelPath {
    pub root: UserRootKind,
    pub rel: PathBuf,
}

impl UserRelPath {
    pub fn new(root: UserRootKind, rel: impl AsRef<Path>) -> Result<Self> {
        let rel = normalize_rel(rel.as_ref())?;
        Ok(Self { root, rel })
    }

    pub fn join_host(&self, home: &Path, data: &Path) -> PathBuf {
        match self.root {
            UserRootKind::Home => home.join(&self.rel),
            UserRootKind::Data => data.join(&self.rel),
        }
    }

    /// Stored form: `home:relative/path` or `data:relative/path`.
    pub fn encode(&self) -> String {
        let kind = match self.root {
            UserRootKind::Home => "home",
            UserRootKind::Data => "data",
        };
        format!("{kind}:{}", self.rel.to_string_lossy().replace('\\', "/"))
    }

    /// Parse a stored root + relative segment. Host absolute paths fail.
    pub fn decode(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if is_host_absolute(raw) {
            bail!("宿主绝对路径不能作为用户路径");
        }
        let (kind, rel) = raw
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("用户路径缺少根类型"))?;
        let root = match kind {
            "home" => UserRootKind::Home,
            "data" => UserRootKind::Data,
            _ => bail!("未知的用户路径根: {kind}"),
        };
        Self::new(root, rel)
    }
}

/// A host absolute path. These must not be opened from a stored user path.
pub fn is_host_absolute(raw: &str) -> bool {
    let trimmed = raw.trim();
    !trimmed.is_empty() && Path::new(trimmed).is_absolute()
}

/// `{app data}/users`, or `{volume}/users` when a deployment overrides the volume root.
pub fn users_root() -> Result<PathBuf> {
    Ok(crate::storage::app_data_dir()?.join("users"))
}

pub fn users_root_under(base: &Path) -> PathBuf {
    base.join("users")
}

/// Host directory for one identity: `{users}/{user_key}` or `{users}/anon/{conversation}`.
pub fn user_tree_dir(
    users_root: &Path,
    session_user_id: &str,
    conversation_id: &str,
) -> Result<PathBuf> {
    let uid = session_user_id.trim();
    if uid.is_empty() {
        let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
        if conv.is_empty() {
            bail!("匿名会话缺少会话 id，不能共用用户目录");
        }
        return Ok(users_root.join("anon").join(conv));
    }
    Ok(users_root.join(user_key(uid)))
}

pub fn user_home_host(
    users_root: &Path,
    session_user_id: &str,
    conversation_id: &str,
) -> Result<PathBuf> {
    Ok(user_tree_dir(users_root, session_user_id, conversation_id)?.join("home"))
}

pub fn user_data_host(
    users_root: &Path,
    session_user_id: &str,
    conversation_id: &str,
) -> Result<PathBuf> {
    Ok(user_tree_dir(users_root, session_user_id, conversation_id)?.join("data"))
}

/// Logged-in users ignore `conversation_id`. Anonymous sessions require it.
pub fn user_home_host_root(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    user_home_host(&users_root()?, session_user_id, conversation_id)
}

pub fn user_data_host_root(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let root = user_data_host(&users_root()?, session_user_id, conversation_id)?;
    ensure_data_layout(&root)?;
    Ok(root)
}

pub fn attachments_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    if conv.is_empty() {
        bail!("attachments require a conversation id");
    }
    let dir = root.join("attachments").join(conv);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create attachments dir {}", dir.display()))?;
    Ok(dir)
}

pub fn media_chat_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    conversation_data_dir(session_user_id, conversation_id, &["media", "chat"])
}

pub fn media_generated_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    conversation_data_dir(session_user_id, conversation_id, &["media", "generated"])
}

pub fn captures_host_dir(
    session_user_id: &str,
    conversation_id: &str,
    date: &str,
) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    let day = crate::storage::sanitize_storage_dir_segment(date.trim());
    if conv.is_empty() || day.is_empty() {
        bail!("captures require a conversation id and date");
    }
    let dir = root.join("captures").join(day).join(conv);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create captures dir {}", dir.display()))?;
    Ok(dir)
}

pub fn plugin_data_host_dir(session_user_id: &str, plugin_id: &str) -> Result<PathBuf> {
    let home = user_home_host_root(session_user_id, "")?;
    let id = crate::storage::sanitize_storage_dir_segment(plugin_id.trim());
    if id.is_empty() {
        bail!("plugin id is required");
    }
    let dir = home.join(".pointer").join("plugin-data").join(id);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create plugin-data dir {}", dir.display()))?;
    Ok(dir)
}

pub fn memories_host_dir(session_user_id: &str, conversation_id: &str) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let dir = root.join("memories");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create memories dir {}", dir.display()))?;
    Ok(dir)
}

pub fn workspace_rel(project_name: &str) -> String {
    let name = crate::storage::sanitize_storage_dir_segment(project_name.trim());
    format!("workspaces/{name}")
}

pub fn worktree_rel(conversation_id: &str) -> String {
    let key = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    format!("worktrees/{key}")
}

pub fn is_under_user_data_host(host: &Path, session_user_id: &str, conversation_id: &str) -> bool {
    match user_data_host_root(session_user_id, conversation_id) {
        Ok(root) => host.starts_with(&root),
        Err(error) => {
            log::warn!(
                "user data root unavailable user={} conversation={conversation_id}: {error:#}",
                session_user_id.trim()
            );
            false
        }
    }
}

/// Result of creating a user tree and copying install-tree packages once.
#[derive(Debug, Clone)]
pub struct UserLayout {
    pub home: PathBuf,
    pub data: PathBuf,
    pub created_home: bool,
    pub skills_copied: usize,
    pub plugins_copied: usize,
    pub agents_copied: usize,
}

/// Create `home/` and `data/` for this identity and copy install-tree packages
/// into `home/.pointer/.../system` when those names are not already present.
pub fn ensure_user_layout(session_user_id: &str, conversation_id: &str) -> Result<UserLayout> {
    ensure_user_layout_at(&users_root()?, session_user_id, conversation_id)
}

pub fn ensure_user_layout_at(
    users_root: &Path,
    session_user_id: &str,
    conversation_id: &str,
) -> Result<UserLayout> {
    let home = user_home_host(users_root, session_user_id, conversation_id)?;
    let data = user_data_host(users_root, session_user_id, conversation_id)?;
    let created_home = !home.exists();
    std::fs::create_dir_all(home.join(".pointer").join("skills").join("system"))
        .with_context(|| format!("create {}", home.display()))?;
    for name in ["org", "user"] {
        std::fs::create_dir_all(home.join(".pointer").join("skills").join(name))?;
    }
    for name in ["system", "user"] {
        std::fs::create_dir_all(home.join(".pointer").join("plugins").join(name))?;
    }
    std::fs::create_dir_all(home.join(".pointer").join("agents").join("system"))?;
    std::fs::create_dir_all(home.join(".pointer").join("plugin-data"))?;
    ensure_data_layout(&data)?;

    let skills_copied = copy_missing_children(
        &install_sources(
            &["POINTER_SERVER_SKILLS_DIR"],
            "skills",
            "/usr/share/pointer-server/skills",
        ),
        &home.join(".pointer").join("skills").join("system"),
    )?;
    let plugins_copied = copy_missing_children(
        &install_sources(
            &["POINTER_SERVER_PLUGINS_DIR"],
            "plugins",
            "/usr/share/pointer-server/plugins",
        ),
        &home.join(".pointer").join("plugins").join("system"),
    )?;
    let agents_copied = copy_missing_children(
        &install_sources(
            &["POINTER_SERVER_AGENTS_DIR"],
            "agents",
            "/usr/share/pointer-server/agents",
        ),
        &home.join(".pointer").join("agents").join("system"),
    )?;

    log::info!(
        "user layout: user={} conversation={} created_home={created_home} skills_copied={skills_copied} plugins_copied={plugins_copied} agents_copied={agents_copied} home={}",
        session_user_id.trim(),
        conversation_id.trim(),
        home.display()
    );
    Ok(UserLayout {
        home,
        data,
        created_home,
        skills_copied,
        plugins_copied,
        agents_copied,
    })
}

fn conversation_data_dir(
    session_user_id: &str,
    conversation_id: &str,
    parts: &[&str],
) -> Result<PathBuf> {
    let root = user_data_host_root(session_user_id, conversation_id)?;
    let conv = crate::storage::sanitize_storage_dir_segment(conversation_id.trim());
    if conv.is_empty() {
        bail!("conversation id is required");
    }
    let mut dir = root;
    for part in parts {
        dir = dir.join(part);
    }
    dir = dir.join(conv);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

fn normalize_rel(rel: &Path) -> Result<PathBuf> {
    if rel.is_absolute() {
        bail!("user relative path must not be absolute: {}", rel.display());
    }
    let mut out = PathBuf::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("user relative path escapes its root: {}", rel.display());
            }
        }
    }
    if out.as_os_str().is_empty() {
        bail!("user relative path is empty");
    }
    Ok(out)
}

fn install_sources(env_keys: &[&str], sibling_name: &str, deb_path: &str) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |path: PathBuf| {
        if !path.is_dir() {
            return;
        }
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.insert(key) {
            sources.push(path);
        }
    };
    for key in env_keys {
        if let Ok(raw) = std::env::var(key) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                push(PathBuf::from(trimmed));
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push(parent.join(sibling_name));
            push(parent.join("../../").join(sibling_name));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        push(cwd.join(sibling_name));
    }
    push(PathBuf::from(deb_path));
    if sources.is_empty() {
        log::warn!("user layout: no install source for {sibling_name}");
    }
    sources
}

/// Copy each child directory that is not already present. Existing names are left untouched.
fn copy_missing_children(sources: &[PathBuf], dest_root: &Path) -> Result<usize> {
    std::fs::create_dir_all(dest_root)
        .with_context(|| format!("create {}", dest_root.display()))?;
    let dest_canon = dest_root
        .canonicalize()
        .unwrap_or_else(|_| dest_root.to_path_buf());
    let mut copied = 0usize;
    let mut seen = std::collections::HashSet::new();
    for source_root in sources {
        let source_canon = source_root
            .canonicalize()
            .unwrap_or_else(|_| source_root.to_path_buf());
        let entries = std::fs::read_dir(source_root)
            .with_context(|| format!("read install source {}", source_root.display()))?;
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                log::warn!(
                    "user layout: skip non-utf8 name under {}",
                    source_root.display()
                );
                continue;
            };
            if name_str.starts_with('.') || !seen.insert(name_str.to_string()) {
                continue;
            }
            let target = dest_root.join(&name);
            if target.exists() {
                continue;
            }
            copy_tree_no_escape(&entry.path(), &target, &source_canon, &dest_canon)?;
            copied += 1;
        }
    }
    Ok(copied)
}

fn copy_tree_no_escape(src: &Path, dst: &Path, source_root: &Path, dest_root: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(src).with_context(|| format!("stat {}", src.display()))?;
    if meta.file_type().is_symlink() {
        let target =
            std::fs::read_link(src).with_context(|| format!("read link {}", src.display()))?;
        let resolved = if target.is_absolute() {
            target
        } else {
            src.parent().unwrap_or(src).join(target)
        };
        let canon = resolved.canonicalize().unwrap_or(resolved);
        if !canon.starts_with(source_root) {
            log::warn!(
                "user layout: skip symlink escaping install tree {} -> {}",
                src.display(),
                canon.display()
            );
            return Ok(());
        }
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
        if !dst
            .canonicalize()
            .unwrap_or_else(|_| dst.to_path_buf())
            .starts_with(dest_root)
        {
            bail!("copy target escaped user home: {}", dst.display());
        }
        for entry in std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
            let entry = entry?;
            copy_tree_no_escape(
                &entry.path(),
                &dst.join(entry.file_name()),
                source_root,
                dest_root,
            )?;
        }
        return Ok(());
    }
    if meta.is_file() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
            if !parent
                .canonicalize()
                .unwrap_or_else(|_| parent.to_path_buf())
                .starts_with(dest_root)
            {
                bail!("copy target escaped user home: {}", dst.display());
            }
        }
        std::fs::copy(src, dst)
            .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_rel_sanitizes() {
        assert_eq!(workspace_rel("Demo App"), "workspaces/Demo_App");
        assert!(!worktree_rel("a:b").contains(':'));
    }

    #[test]
    fn relative_path_rejects_parent() {
        assert!(UserRelPath::new(UserRootKind::Data, "../x").is_err());
        assert!(UserRelPath::new(UserRootKind::Data, "/data/memories").is_err());
        let rel = UserRelPath::new(UserRootKind::Data, "memories/a.md").unwrap();
        assert_eq!(
            rel.join_host(Path::new("/home"), Path::new("/data")),
            PathBuf::from("/data/memories/a.md")
        );
        assert_eq!(rel.encode(), "data:memories/a.md");
        assert_eq!(UserRelPath::decode("data:memories/a.md").unwrap(), rel);
        assert!(UserRelPath::decode("/Users/someone/secret.pdf").is_err());
        assert!(is_host_absolute("/etc/passwd"));
    }

    #[test]
    fn anonymous_trees_are_per_conversation() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let a = user_home_host_root("", "conv-a").unwrap();
        let b = user_home_host_root("", "conv-b").unwrap();
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("anon"));
        assert!(user_home_host_root("", "").is_err());
    }

    #[test]
    fn ensure_layout_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let users = root.path().join("users");
        let first = ensure_user_layout_at(&users, "alice", "c-1").unwrap();
        let marker = first
            .home
            .join(".pointer")
            .join("skills")
            .join("system")
            .join("custom");
        std::fs::create_dir_all(&marker).unwrap();
        std::fs::write(marker.join("keep.txt"), "edited").unwrap();
        let second = ensure_user_layout_at(&users, "alice", "c-1").unwrap();
        assert_eq!(first.home, second.home);
        assert_eq!(
            std::fs::read_to_string(marker.join("keep.txt")).unwrap(),
            "edited"
        );
        assert!(first.data.join("memories").is_dir() || second.data.join("workspaces").is_dir());
    }

    #[test]
    fn media_dirs_are_created_under_user_data() {
        let _guard = crate::storage::test_app_data_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        crate::storage::set_test_app_data_dir(temp.path().to_path_buf());
        let dir = media_chat_host_dir("alice", "conv-1").unwrap();
        assert!(dir.is_dir());
        assert!(dir.to_string_lossy().contains("users"));
        assert!(dir.to_string_lossy().contains("media"));
    }
}
