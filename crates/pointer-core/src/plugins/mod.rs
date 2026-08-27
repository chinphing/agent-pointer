//! Pointer 原生插件机制（设计稿 docs/plans/plugin-system-plan.md P1）。
//!
//! - [`manifest`]：`pointer-plugin.toml` 解析与校验（唯一运行时格式）。
//! - [`registry`]：发现 / 授权 / 状态机（P1）。
//! - [`tool_provider`]：进程外工具执行载体（sidecar，stdio + JSON 协议）。
//! - [`importer`]：Codex / Claude 插件目录 → 原生格式的一次性导入转换。
//! - [`agents_md`]：`~/.pointer/AGENTS.md` + git 根沿工作区路径拼接（不扫旁支），
//!   每轮写入 system cacheable `# Project Context`。
//!
//! 设计铁律：发现 ≠ 执行；授权留痕（manifest + 能力单元文件清单哈希）；
//! 插件工具与内置工具走同一审批链路。

pub mod activation;
pub mod agents_md;
pub mod external_probe;
pub mod hooks;
pub mod importer;
pub mod manifest;
pub mod mcp;
pub mod registry;
pub mod tool_provider;

#[cfg(test)]
pub mod e2e_tests;

/// `pointer-plugin.toml` 文件名。
pub const PLUGIN_MANIFEST_FILE: &str = "pointer-plugin.toml";
/// `~/.pointer/plugins`（用户级原生插件）。
pub const USER_PLUGINS_DIR_NAME: &str = "plugins";
/// 工作区级插件目录 `.pointer/plugins`（随仓库分发，需项目显式启用）。
pub const WORKSPACE_PLUGINS_DIR_NAME: &str = ".pointer/plugins";

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `~/.pointer` 共享主目录（与 skills 同一 home）。
/// 测试/部署可用 `POINTER_HOME` 覆盖整个 pointer home（避免污染真实用户目录）。
pub fn pointer_home_dir() -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("POINTER_HOME") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let dir = PathBuf::from(trimmed);
            std::fs::create_dir_all(&dir)?;
            return Ok(dir);
        }
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法解析用户主目录"))?;
    let dir = home.join(".pointer");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 用户级插件根目录 `~/.pointer/plugins`（不存在时创建）。
pub fn user_plugins_dir() -> Result<PathBuf> {
    let dir = pointer_home_dir()?.join(USER_PLUGINS_DIR_NAME);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 工作区级插件根目录 `<workspace>/.pointer/plugins`（不存在时创建）。
pub fn workspace_plugins_dir() -> Result<PathBuf> {
    let root = crate::tools::file::resolve_tool_workspace_root()?;
    let dir = root.join(WORKSPACE_PLUGINS_DIR_NAME);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 插件目录在用户级 / 工作区级根目录下的直接子目录（kebab-case 反向域名 id）。
pub fn discover_plugin_dirs() -> Result<Vec<(PathBuf, bool)>> {
    // (dir, is_user_level)
    let mut out: Vec<(PathBuf, bool)> = Vec::new();
    let user = user_plugins_dir()?;
    for entry in std::fs::read_dir(&user)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() && path.join(PLUGIN_MANIFEST_FILE).is_file() {
            out.push((path, true));
        }
    }
    if let Ok(workspace) = workspace_plugins_dir() {
        for entry in std::fs::read_dir(&workspace)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join(PLUGIN_MANIFEST_FILE).is_file() {
                out.push((path, false));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 递归收集插件目录下全部文件（相对路径 → 内容哈希），用于授权留痕。
pub fn fingerprint_plugin_files(plugin_dir: &Path) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    collect_dir_fingerprint(plugin_dir, plugin_dir, &mut out)?;
    Ok(out)
}

fn collect_dir_fingerprint(
    base: &Path,
    dir: &Path,
    out: &mut HashMap<String, String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_dir_fingerprint(base, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| anyhow!("插件文件路径异常: {e}"))?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let bytes = std::fs::read(&path)?;
            out.insert(rel_str, sha256_hex(&bytes));
        }
    }
    Ok(())
}

/// SHA-256 hex（用于 manifest / 文件清单哈希留痕）。
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 解析 `pointer-plugin.toml` 并做结构校验；返回 `(PluginManifest, 原始文本)`。
pub fn load_manifest(dir: &Path) -> Result<(manifest::PluginManifest, String)> {
    let path = dir.join(PLUGIN_MANIFEST_FILE);
    let raw = std::fs::read_to_string(&path).map_err(|e| anyhow!("读取 {path:?} 失败: {e}"))?;
    let parsed = manifest::parse(&raw)?;
    Ok((parsed, raw))
}

/// 当前毫秒时间戳（授权时间戳等）。
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
