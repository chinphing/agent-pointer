//! 容器把客人路径翻译成宿主路径之后，用这个类型读写那一条宿主路径。
//!
//! 它不是会话环境，不能放进线程槽，也没有整机路径表。调用方必须先完成翻译。

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use super::{DirEntry, FileMetadata, WalkEntry, WalkOptions};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct VolumeIo;

impl VolumeIo {
    pub(crate) fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        std::fs::read(path).with_context(|| format!("读取失败: {}", path.display()))
    }

    pub(crate) fn write_file(&self, path: &Path, data: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("无法创建目录: {}", parent.display()))?;
            }
        }
        std::fs::write(path, data).with_context(|| format!("写入失败: {}", path.display()))
    }

    pub(crate) fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        let entries =
            std::fs::read_dir(path).with_context(|| format!("无法列出目录: {}", path.display()))?;
        for entry in entries.flatten() {
            let file_type = entry.file_type().ok();
            out.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                is_dir: file_type.as_ref().is_some_and(|t| t.is_dir()),
                is_symlink: file_type.as_ref().is_some_and(|t| t.is_symlink()),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub(crate) fn metadata(&self, path: &Path) -> Result<FileMetadata> {
        let meta = std::fs::symlink_metadata(path)
            .with_context(|| format!("读取元数据失败: {}", path.display()))?;
        let is_symlink = meta.file_type().is_symlink();
        let resolved = if is_symlink {
            std::fs::metadata(path).ok()
        } else {
            None
        };
        let effective = resolved.as_ref().unwrap_or(&meta);
        Ok(FileMetadata {
            is_file: effective.is_file(),
            is_dir: effective.is_dir(),
            is_symlink,
            len: effective.len(),
        })
    }

    pub(crate) fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        std::fs::canonicalize(path).with_context(|| format!("无法解析路径: {}", path.display()))
    }

    pub(crate) fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).with_context(|| format!("无法创建目录: {}", path.display()))
    }

    pub(crate) fn read_to_string(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).with_context(|| format!("读取失败: {}", path.display()))
    }

    pub(crate) fn open_reader(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>> {
        let file =
            std::fs::File::open(path).with_context(|| format!("读取失败: {}", path.display()))?;
        Ok(Box::new(file))
    }

    pub(crate) fn walk(&self, root: &Path, opts: WalkOptions) -> Result<Vec<WalkEntry>> {
        let mut builder = ignore::WalkBuilder::new(root);
        builder.max_depth(Some(opts.max_depth.max(1)));
        builder.hidden(!opts.include_hidden);
        builder.git_ignore(opts.respect_gitignore);
        builder.git_global(opts.respect_gitignore);
        builder.git_exclude(opts.respect_gitignore);
        builder.ignore(opts.respect_gitignore);
        builder.parents(opts.respect_gitignore);
        let mut out = Vec::new();
        for entry in builder.build().filter_map(|e| e.ok()) {
            if entry.depth() == 0 {
                continue;
            }
            let path = entry.path();
            if opts.exclude_git_metadata
                && path.to_string_lossy().replace('\\', "/").contains("/.git/")
            {
                continue;
            }
            let file_type = entry.file_type();
            out.push(WalkEntry {
                path: path.to_path_buf(),
                is_dir: file_type.is_some_and(|t| t.is_dir()),
                is_symlink: file_type.is_some_and(|t| t.is_symlink()),
            });
        }
        Ok(out)
    }

    pub(crate) fn remove_file(&self, path: &Path) -> Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!("删除失败 {}: {e}", path.display())),
        }
    }

    pub(crate) fn remove_dir_all(&self, path: &Path) -> Result<()> {
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow!("删除目录失败 {}: {e}", path.display())),
        }
    }

    pub(crate) fn set_executable(&self, path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("设置权限失败: {}", path.display()))?;
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
        Ok(())
    }
}
