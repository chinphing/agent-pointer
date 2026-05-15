//! Optional workspace file `.pointer/lint.toml` for extra shell-based lint commands.

use anyhow::{anyhow, Result};
use log::warn;
use serde::Deserialize;
use std::fs;
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

pub fn load_lint_config(root: &Path) -> Option<LintFile> {
    let path = config_file_path(root);
    if !path.is_file() {
        return None;
    }
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            warn!("read_lints: failed to read {}: {e}", path.display());
            return None;
        }
    };
    match toml::from_str::<LintFile>(&raw) {
        Ok(f) => Some(f),
        Err(e) => {
            warn!("read_lints: invalid TOML in {}: {e}", path.display());
            None
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
