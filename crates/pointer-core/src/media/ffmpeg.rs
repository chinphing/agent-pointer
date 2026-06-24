use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW);
        return cmd;
    }
    #[cfg(not(windows))]
    Command::new(program)
}

/// Returns true when both ffmpeg and ffprobe resolve and respond to `-version`.
pub fn ffmpeg_available() -> bool {
    matches!(probe_ffmpeg_tools().status, FfmpegToolStatus::Ready)
}

pub fn resolve_ffmpeg() -> Option<PathBuf> {
    resolve_tool("ffmpeg")
}

pub fn resolve_ffprobe() -> Option<PathBuf> {
    resolve_tool("ffprobe")
}

pub fn resolve_pdftoppm() -> Option<PathBuf> {
    resolve_tool("pdftoppm")
}

pub fn pdftoppm_available() -> bool {
    resolve_pdftoppm().is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FfmpegToolStatus {
    Ready,
    NotFound,
    Partial,
    NotExecutable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegToolProbe {
    pub status: FfmpegToolStatus,
    pub ffmpeg_available: bool,
    pub ffprobe_available: bool,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub detail: Option<String>,
}

pub fn probe_ffmpeg_tools() -> FfmpegToolProbe {
    let ffmpeg_path = resolve_tool("ffmpeg");
    let ffprobe_path = resolve_tool("ffprobe");
    let ffmpeg_found = ffmpeg_path.is_some();
    let ffprobe_found = ffprobe_path.is_some();

    if !ffmpeg_found && !ffprobe_found {
        return FfmpegToolProbe {
            status: FfmpegToolStatus::NotFound,
            ffmpeg_available: false,
            ffprobe_available: false,
            ffmpeg_path: None,
            ffprobe_path: None,
            detail: Some("未找到 ffmpeg 与 ffprobe 可执行文件".into()),
        };
    }

    if !ffmpeg_found || !ffprobe_found {
        let missing = if !ffmpeg_found { "ffmpeg" } else { "ffprobe" };
        return FfmpegToolProbe {
            status: FfmpegToolStatus::Partial,
            ffmpeg_available: false,
            ffprobe_available: false,
            ffmpeg_path: ffmpeg_path.as_ref().map(|p| path_display(p.as_path())),
            ffprobe_path: ffprobe_path.as_ref().map(|p| path_display(p.as_path())),
            detail: Some(format!("仅找到部分组件，缺少 {missing}")),
        };
    }

    let ffmpeg_path = ffmpeg_path.expect("ffmpeg path");
    let ffprobe_path = ffprobe_path.expect("ffprobe path");
    let ffmpeg_ok = tool_responds_to_version(&ffmpeg_path);
    let ffprobe_ok = tool_responds_to_version(&ffprobe_path);
    if !ffmpeg_ok || !ffprobe_ok {
        let broken = if !ffmpeg_ok { "ffmpeg" } else { "ffprobe" };
        return FfmpegToolProbe {
            status: FfmpegToolStatus::NotExecutable,
            ffmpeg_available: false,
            ffprobe_available: false,
            ffmpeg_path: Some(path_display(ffmpeg_path.as_path())),
            ffprobe_path: Some(path_display(ffprobe_path.as_path())),
            detail: Some(format!(
                "{broken} 已找到但执行失败（权限、架构或安装损坏）"
            )),
        };
    }

    FfmpegToolProbe {
        status: FfmpegToolStatus::Ready,
        ffmpeg_available: true,
        ffprobe_available: true,
        ffmpeg_path: Some(path_display(ffmpeg_path.as_path())),
        ffprobe_path: Some(path_display(ffprobe_path.as_path())),
        detail: None,
    }
}

fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn tool_responds_to_version(path: &Path) -> bool {
    hidden_command(path)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn resolve_tool(name: &str) -> Option<PathBuf> {
    for dir in extra_search_dirs() {
        let candidate = dir.join(tool_filename(name));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(path) = resolve_via_path_lookup(name) {
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn tool_filename(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Walk `PATH` in-process (Windows-friendly: avoids spawning `where.exe` on every settings open).
fn resolve_via_path_lookup(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let path_env = std::env::var("PATH")
            .or_else(|_| std::env::var("Path"))
            .unwrap_or_default();
        for dir in path_env.split(';') {
            let dir = dir.trim();
            if dir.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(dir).join(tool_filename(name));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        return None;
    }
    #[cfg(not(windows))]
    {
        let output = Command::new("which").arg(name).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text.lines().next()?.trim();
        if line.is_empty() {
            None
        } else {
            Some(PathBuf::from(line))
        }
    }
}

fn extra_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/usr/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(windows)]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(pf).join("ffmpeg").join("bin"));
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            dirs.push(PathBuf::from(pf86).join("ffmpeg").join("bin"));
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_returns_structured_status() {
        let probe = probe_ffmpeg_tools();
        match probe.status {
            FfmpegToolStatus::Ready => {
                assert!(probe.ffmpeg_available);
                assert!(probe.ffprobe_available);
            }
            FfmpegToolStatus::NotFound => {
                assert!(!probe.ffmpeg_available);
            }
            FfmpegToolStatus::Partial | FfmpegToolStatus::NotExecutable => {
                assert!(!probe.ffmpeg_available);
                assert!(probe.detail.is_some());
            }
        }
    }
}
