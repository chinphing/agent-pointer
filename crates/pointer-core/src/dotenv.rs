//! Parse `.env` files and apply variables as **supplementary** child-process env
//! (never override keys already set in the host process).

use anyhow::{anyhow, Result};
use log::warn;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Parse a dotenv file into key/value pairs. Later duplicate keys in the same file win.
pub fn parse_dotenv_bytes(content: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(content);
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = parse_dotenv_line(line) else {
            continue;
        };
        out.insert(key, value);
    }
    out
}

/// Load one or more `.env` files (in order) and apply only keys missing from the host process.
pub fn apply_supplemental_env_files(
    cmd: &mut Command,
    env_files: &[PathBuf],
) -> Vec<String> {
    let mut merged: HashMap<String, String> = HashMap::new();
    let mut loaded = Vec::new();

    for path in env_files {
        match std::fs::read(path) {
            Ok(bytes) => {
                for (k, v) in parse_dotenv_bytes(&bytes) {
                    merged.insert(k, v);
                }
                loaded.push(path.display().to_string());
            }
            Err(e) => {
                warn!(
                    "dotenv: failed to read {}: {e}",
                    path.display()
                );
            }
        }
    }

    for (key, value) in merged {
        if std::env::var(&key).is_ok() {
            continue;
        }
        cmd.env(key, value);
    }

    loaded
}

/// Default user env file under the app data directory (`PointerApp/.env`).
pub fn default_user_env_file() -> Option<PathBuf> {
    match crate::storage::user_env_file_path() {
        Ok(path) if path.is_file() => Some(path),
        Ok(_) => None,
        Err(e) => {
            warn!("dotenv: app data dir unavailable for default .env: {e}");
            None
        }
    }
}

/// Resolve env file paths from tool args (`envFiles` string or array of strings).
pub fn parse_env_file_args(args: &serde_json::Value) -> Vec<String> {
    let Some(value) = args.get("envFiles") else {
        return vec![];
    };
    match value {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                vec![]
            } else {
                vec![s.to_string()]
            }
        }
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => vec![],
    }
}

/// Resolve a user-supplied env file path against `cwd` (or workspace root fallback).
pub fn resolve_env_file_path(
    raw: &str,
    cwd: Option<&Path>,
    workspace_root: Option<&Path>,
) -> Result<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(anyhow!("env file path is empty"));
    }
    let path = Path::new(raw);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = cwd
            .or(workspace_root)
            .ok_or_else(|| anyhow!("cannot resolve relative env file path (no cwd): {raw}"))?;
        base.join(path)
    };
    if !full.is_file() {
        return Err(anyhow!("env file not found: {raw}"));
    }
    Ok(full.canonicalize().unwrap_or(full))
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line).trim();
    let (key_part, value_part) = split_key_value(line)?;
    let key = key_part.trim();
    if key.is_empty() || !is_valid_env_key(key) {
        return None;
    }
    let value = parse_dotenv_value(value_part.trim());
    Some((key.to_string(), value))
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '=' if !in_single && !in_double => {
                return Some((&line[..i], &line[i + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn parse_dotenv_value(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        return unescape_quoted(&raw[1..raw.len() - 1], raw.starts_with('"'));
    }
    raw.to_string()
}

fn unescape_quoted(inner: &str, double: bool) -> String {
    if !double {
        return inner.to_string();
    }
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_test_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn parse_basic_and_export() {
        let content = b"# comment\nFOO=bar\nexport BAZ=qux\nEMPTY=\n";
        let m = parse_dotenv_bytes(content);
        assert_eq!(m.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(m.get("BAZ").map(|s| s.as_str()), Some("qux"));
        assert_eq!(m.get("EMPTY").map(|s| s.as_str()), Some(""));
        assert!(!m.contains_key("# comment"));
    }

    #[test]
    fn parse_quoted_values() {
        let content = b"DOUBLE=\"hello world\"\nSINGLE='a=b'\nESC=\"line\\n2\"\n";
        let m = parse_dotenv_bytes(content);
        assert_eq!(m.get("DOUBLE").map(|s| s.as_str()), Some("hello world"));
        assert_eq!(m.get("SINGLE").map(|s| s.as_str()), Some("a=b"));
        assert_eq!(m.get("ESC").map(|s| s.as_str()), Some("line\n2"));
    }

    #[test]
    fn supplemental_does_not_override_process_env() {
        let _guard = env_test_guard();
        let key = "POINTER_DOTENV_TEST_ONLY";
        std::env::set_var(key, "from_process");
        let mut cmd = Command::new("sh");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, format!("{key}=from_file\nOTHER_FROM_FILE=1\n")).unwrap();
        let loaded = apply_supplemental_env_files(&mut cmd, &[path]);
        assert_eq!(loaded.len(), 1);
        std::env::remove_var(key);
    }

    #[test]
    fn parse_env_file_args_accepts_array() {
        let args = serde_json::json!({
            "envFiles": [".env", ".env.local", "secrets.env"]
        });
        let paths = parse_env_file_args(&args);
        assert_eq!(paths, vec![".env", ".env.local", "secrets.env"]);
    }

    #[test]
    fn parse_env_file_args_accepts_single_string() {
        let args = serde_json::json!({ "envFiles": ".env" });
        assert_eq!(parse_env_file_args(&args), vec![".env"]);
    }

    #[test]
    fn parse_env_file_args_empty_when_omitted() {
        let args = serde_json::json!({ "command": "echo hi" });
        assert!(parse_env_file_args(&args).is_empty());
    }
}
