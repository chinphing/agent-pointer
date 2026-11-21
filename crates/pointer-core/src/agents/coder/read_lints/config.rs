//! Optional workspace file `.pointer/lint.toml` for extra shell-based lint commands.

use anyhow::{anyhow, Result};
use log::warn;
use serde::Deserialize;

use std::path::Path;

use super::detect::config_file_path;

#[derive(Debug, Clone, Deserialize)]
pub struct LintFile {
    #[serde(default)]
    pub commands: Vec<LintCommandEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LintCommandEntry {
    /// One shell line (same as typed in project root), e.g. `mvn -q checkstyle:check`.
    pub shell: String,
    /// `eslint-json` | `ruff-json` | `cargo-json-lines` | `maven-log` | `text-on-failure` (default).
    #[serde(default = "default_parser")]
    pub parser: String,
}

fn default_parser() -> String {
    "text-on-failure".to_string()
}

pub fn load_lint_config(root: &Path) -> Result<Option<LintFile>> {
    let path = config_file_path(root);
    // 走执行环境：headless 档位下配置文件在沙箱工作区里，宿主路径不存在。
    // 工具入口（`run_read_lints`）在分发点已绑定环境，这里要求显式绑定：
    // 拿不到就报错，不按部署档位回落到 `DeniedEnv` / 宿主。
    let env = crate::env::require_env()?;
    if !env.exists(&path) {
        return Ok(None);
    }
    let raw = match env.read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            warn!("read_lints: failed to read {}: {e}", path.display());
            return Ok(None);
        }
    };
    match toml::from_str::<LintFile>(&raw) {
        Ok(f) => Ok(Some(f)),
        Err(e) => {
            warn!("read_lints: invalid TOML in {}: {e}", path.display());
            Ok(None)
        }
    }
}

pub fn validate_parser(name: &str) -> Result<()> {
    match name {
        "eslint-json" | "oxlint-json" | "ruff-json" | "cargo-json-lines" | "maven-log" | "text-on-failure" => Ok(()),
        _ => Err(anyhow!(
            "未知 parser {:?}；允许: eslint-json, oxlint-json, ruff-json, cargo-json-lines, maven-log, text-on-failure",
            name
        )),
    }
}
