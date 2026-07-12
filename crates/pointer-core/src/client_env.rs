//! Client environment snapshot sent with desktop OAuth login (token exchange).

use serde::{Deserialize, Serialize};
use std::env;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginClientEnv {
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    pub arch: String,
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

pub fn collect_login_client_env() -> LoginClientEnv {
    LoginClientEnv {
        platform: env::consts::OS.to_string(),
        os_version: detect_os_version(),
        arch: env::consts::ARCH.to_string(),
        app_version: app_version(),
        locale: env::var("LANG")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
    }
}

pub fn app_version() -> String {
    option_env!("POINTER_APP_VERSION")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

fn detect_os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    #[cfg(target_os = "windows")]
    {
        return hidden_command("cmd")
            .args(["/C", "ver"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().replace('\r', "").replace('\n', " "))
            .filter(|s| !s.is_empty());
    }
    #[cfg(target_os = "linux")]
    {
        return read_linux_os_version();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn read_linux_os_version() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    let mut pretty: Option<String> = None;
    let mut version_id: Option<String> = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
            pretty = Some(trim_os_release_value(v));
        } else if let Some(v) = line.strip_prefix("VERSION_ID=") {
            version_id = Some(trim_os_release_value(v));
        }
    }
    pretty.or(version_id)
}

#[cfg(target_os = "linux")]
fn trim_os_release_value(raw: &str) -> String {
    raw.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_login_client_env_has_core_fields() {
        let env = collect_login_client_env();
        assert!(!env.platform.is_empty());
        assert!(!env.arch.is_empty());
        assert!(!env.app_version.is_empty());
    }
}
