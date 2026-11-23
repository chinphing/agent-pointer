//! 聊天之外对某个用户 home/data 的读写。不能执行命令，不能放进会话线程槽。

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::user_data_paths::{ensure_user_layout, UserLayout};

#[derive(Debug, Clone)]
pub struct UserTree {
    pub home: PathBuf,
    pub data: PathBuf,
}

impl UserTree {
    pub fn open(session_user_id: &str, conversation_id: &str) -> Result<Self> {
        let layout = ensure_user_layout(session_user_id, conversation_id)?;
        Self::from_layout(&layout)
    }

    pub fn from_layout(layout: &UserLayout) -> Result<Self> {
        Ok(Self {
            home: layout.home.clone(),
            data: layout.data.clone(),
        })
    }

    /// 相对段必须落在 home 或 data 里。`..` 和绝对路径拒绝。
    pub fn path_in_home(&self, rel: &Path) -> Result<PathBuf> {
        self.join_checked(&self.home, rel)
    }

    pub fn path_in_data(&self, rel: &Path) -> Result<PathBuf> {
        self.join_checked(&self.data, rel)
    }

    fn join_checked(&self, root: &Path, rel: &Path) -> Result<PathBuf> {
        if rel.is_absolute() {
            bail!("用户树路径不能是绝对路径: {}", rel.display());
        }
        for component in rel.components() {
            if matches!(component, Component::ParentDir) {
                bail!("用户树路径不能包含 ..: {}", rel.display());
            }
        }
        let path = root.join(rel);
        let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if path.exists() {
            let canon = path
                .canonicalize()
                .with_context(|| format!("canonicalize {}", path.display()))?;
            if !canon.starts_with(&root_canon) {
                bail!("路径超出用户目录: {}", canon.display());
            }
            return Ok(canon);
        }
        if let Some(parent) = path.parent() {
            if parent.exists() {
                let parent_canon = parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf());
                if !parent_canon.starts_with(&root_canon) {
                    bail!("路径超出用户目录: {}", path.display());
                }
            }
        }
        Ok(path)
    }
}
