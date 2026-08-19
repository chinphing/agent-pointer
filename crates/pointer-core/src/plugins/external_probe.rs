//! 外部插件来源探测（设计稿 §4.2「导入来源目录」）。
//!
//! 只读探测本机其他 Agent（Claude Code / Codex）的插件目录，供「导入」入口
//! 列出可导入项；**不直接生效**（导入走 `importer::import_claude_plugin` 转原生）。
//!
//! P1 高置信度面（风险表 §10）：Claude 插件包（`.claude-plugin/plugin.json`）；
//! Codex 插件包格式官方文档 403，等官方可达后再补。

use crate::plugins::importer::{import_claude_plugin, ImportReport};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 可导入的外部插件来源。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalPluginSource {
    /// 来源 id（稳定；用于 `import_external_plugin`）。
    pub id: String,
    pub label: String,
    pub path: String,
    #[serde(rename = "pluginName")]
    pub plugin_name: String,
    #[serde(rename = "pluginVersion")]
    pub plugin_version: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalPluginsProbeResult {
    pub sources: Vec<ExternalPluginSource>,
    #[serde(rename = "total")]
    pub total: u32,
}

/// 探测 `~/.claude/plugins` 及其子目录（含 `.claude-plugin/plugin.json` 或根 `plugin.json`）。
pub fn probe_external_plugin_sources() -> Result<ExternalPluginsProbeResult> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("无法解析用户主目录"))?;
    let mut sources = Vec::new();

    let mut push_source = |dir: &Path| {
        let Some(plugin_json) = find_claude_manifest_in(dir) else {
            return;
        };
        let Ok(raw) = fs::read_to_string(&plugin_json) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };
        let plugin_name = value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if plugin_name.is_empty() {
            return;
        }
        let id = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("claude-plugin")
            .to_string();
        let version = value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        let description = value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        sources.push(ExternalPluginSource {
            id: format!("claude:{id}"),
            label: format!("Claude Code 插件：{plugin_name}"),
            path: dir.to_string_lossy().to_string(),
            plugin_name,
            plugin_version: version,
            description,
        });
    };

    // ~/.claude/plugins 根（本身含 plugin.json）
    let claude_plugins = home.join(".claude").join("plugins");
    if claude_plugins.is_dir() {
        push_source(&claude_plugins);
        // 子目录：每个子插件
        for entry in fs::read_dir(&claude_plugins)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                push_source(&path);
            }
        }
    }
    // ~/.claude 根（单插件形式 .claude-plugin/plugin.json）
    let claude_root = home.join(".claude");
    if claude_root.is_dir() && claude_root.join(".claude-plugin/plugin.json").is_file() {
        push_source(&claude_root);
    }

    // Codex 插件目录（官方格式未定；先探测含 plugin.json 的目录，P1 宽容处理）
    let codex_home = codex_home_dir(&home);
    let codex_plugins = codex_home.join("plugins");
    if codex_plugins.is_dir() {
        for entry in fs::read_dir(&codex_plugins).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir()
                && (path.join("plugin.json").is_file() || path.join("plugin.toml").is_file())
            {
                push_source(&path);
            }
        }
    }

    sources.sort_by(|a, b| a.id.cmp(&b.id));
    let total = sources.len() as u32;
    Ok(ExternalPluginsProbeResult { sources, total })
}

/// 按来源 id 导入一个外部插件（转原生格式并装配）。
pub fn import_external_plugin(source_id: &str) -> Result<ImportReport> {
    let probe = probe_external_plugin_sources()?;
    let source = probe
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| anyhow!("未找到可导入来源: {source_id}"))?;
    let dir = PathBuf::from(&source.path);
    if !dir.is_dir() {
        return Err(anyhow!("来源目录不存在: {}", dir.display()));
    }
    let target_root = crate::plugins::user_plugins_dir()?;
    import_claude_plugin(&dir, &target_root)
}

fn codex_home_dir(home: &Path) -> PathBuf {
    if let Ok(codex_home) = std::env::var("CODEX_HOME") {
        let trimmed = codex_home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    home.join(".codex")
}

/// 在目录内定位 Claude plugin.json（`.claude-plugin/plugin.json` 或根 `plugin.json`）。
fn find_claude_manifest_in(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        dir.join(".claude-plugin").join("plugin.json"),
        dir.join("plugin.json"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_claude_plugin(root: &Path, name: &str, label: &str) {
        let dir = root.join(name);
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(
            dir.join(".claude-plugin/plugin.json"),
            format!(r#"{{"name": "{label}", "version": "1.0.0", "description": "测试插件"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn probes_claude_plugins_directory() {
        // 用临时 HOME 结构验证：把 ~/.claude 换成临时目录不可行（dirs::home_dir），
        // 因此直接验证 find_claude_manifest_in + 探测核心路径的目录结构判断。
        let tmp = tempfile::tempdir().unwrap();
        write_claude_plugin(tmp.path(), "demo", "Demo Plugin");
        let found = find_claude_manifest_in(&tmp.path().join("demo"));
        assert!(found.is_some());

        let manifest = found.unwrap();
        let raw = fs::read_to_string(manifest).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["name"], "Demo Plugin");
    }

    #[test]
    fn finds_root_plugin_json_too() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("plugin.json"),
            r#"{"name": "Root", "version": "0.1.0"}"#,
        )
        .unwrap();
        assert!(find_claude_manifest_in(tmp.path()).is_some());
    }

    #[test]
    fn import_external_plugin_missing_source_errors() {
        let err = import_external_plugin("claude:not-exists")
            .unwrap_err()
            .to_string();
        assert!(err.contains("未找到可导入来源"));
    }
}
