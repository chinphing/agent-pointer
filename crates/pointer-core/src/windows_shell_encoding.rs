//! Windows shell UTF-8 setup shared by `terminal`, elevated runs, and lint subprocesses.

#![cfg_attr(not(windows), allow(dead_code))]

use std::collections::HashMap;

/// PowerShell preamble: console input/output and pipeline encoding → UTF-8 (no BOM).
pub const POWERSHELL_UTF8_PREAMBLE: &str = "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); $OutputEncoding = [System.Text.UTF8Encoding]::new($false); ";

/// Multi-line block for `.ps1` job scripts (elevated runs).
pub const POWERSHELL_UTF8_SETUP_BLOCK: &str = "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)";

pub fn command_already_sets_utf8(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.contains("inputencoding")
        || lower.contains("outputencoding")
        || lower.contains("utf8encoding")
        || lower.starts_with("chcp ")
        || lower.contains("chcp 65001")
}

pub fn wrap_powershell_command(command: &str) -> String {
    if command_already_sets_utf8(command) {
        return command.to_string();
    }
    format!("{POWERSHELL_UTF8_PREAMBLE}{command}")
}

pub fn prefix_cmd_utf8_codepage(script: &str) -> String {
    if command_already_sets_utf8(script) {
        return script.to_string();
    }
    format!("chcp 65001>nul & {script}")
}

/// When the model passes `-Command` / `-c`, prepend UTF-8 encoding setup to that argument.
pub fn wrap_powershell_args_with_utf8(mut args: Vec<String>) -> Vec<String> {
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].to_ascii_lowercase();
        if flag == "-command" || flag == "-c" {
            if let Some(cmd) = args.get_mut(i + 1) {
                *cmd = wrap_powershell_command(cmd);
            }
            break;
        }
        i += 1;
    }
    args
}

/// Default UTF-8 env for terminal child processes on Windows.
pub fn apply_windows_utf8_child_env(env: &mut HashMap<String, String>) {
    env.entry("PYTHONIOENCODING".into())
        .or_insert_with(|| "utf-8".into());
    env.entry("PYTHONUTF8".into())
        .or_insert_with(|| "1".into());
    env.entry("JAVA_TOOL_OPTIONS".into())
        .or_insert_with(|| "-Dfile.encoding=UTF-8".into());
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn wrap_powershell_command_prepends_utf8_preamble() {
        let wrapped = wrap_powershell_command("lark-cli config init --new");
        assert!(wrapped.starts_with(POWERSHELL_UTF8_PREAMBLE));
        assert!(wrapped.ends_with("lark-cli config init --new"));
        assert!(wrapped.contains("InputEncoding"));
    }

    #[test]
    fn wrap_powershell_command_skips_when_already_set() {
        let cmd = "$OutputEncoding = [System.Text.UTF8Encoding]::new($false); foo";
        assert_eq!(wrap_powershell_command(cmd), cmd);
    }

    #[test]
    fn prefix_cmd_utf8_codepage_prepends_chcp() {
        assert_eq!(
            prefix_cmd_utf8_codepage("echo %PATH%"),
            "chcp 65001>nul & echo %PATH%"
        );
    }

    #[test]
    fn prefix_cmd_utf8_codepage_skips_when_already_set() {
        let script = "chcp 65001>nul & dir";
        assert_eq!(prefix_cmd_utf8_codepage(script), script);
    }

    #[test]
    fn wrap_powershell_args_with_utf8_wraps_command_flag() {
        let args = wrap_powershell_args_with_utf8(vec![
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "choco -v".to_string(),
        ]);
        assert!(args[2].starts_with(POWERSHELL_UTF8_PREAMBLE));
        assert!(args[2].ends_with("choco -v"));
    }

    #[test]
    fn apply_windows_utf8_child_env_sets_defaults() {
        let mut env = HashMap::new();
        apply_windows_utf8_child_env(&mut env);
        assert_eq!(env.get("PYTHONIOENCODING").map(String::as_str), Some("utf-8"));
        assert_eq!(env.get("PYTHONUTF8").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("JAVA_TOOL_OPTIONS").map(String::as_str),
            Some("-Dfile.encoding=UTF-8")
        );
    }

    #[test]
    fn apply_windows_utf8_child_env_does_not_override_existing() {
        let mut env = HashMap::from([("PYTHONIOENCODING".into(), "latin-1".into())]);
        apply_windows_utf8_child_env(&mut env);
        assert_eq!(env.get("PYTHONIOENCODING").map(String::as_str), Some("latin-1"));
    }
}
