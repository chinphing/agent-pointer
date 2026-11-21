use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

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
            detail: Some(format!("{broken} 已找到但执行失败（权限、架构或安装损坏）")),
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
    // 走 exec：local 用绝对路径，headless 用工具名在沙箱里解析。
    let program = path.to_string_lossy().into_owned();
    crate::media::exec::run(&program, &["-version".to_string()])
        .map(|o| o.success())
        .unwrap_or(false)
}

fn resolve_tool(name: &str) -> Option<PathBuf> {
    // headless/desktop：媒体工具装在沙箱镜像里，返回名字让 `exec` 走沙箱 PATH；
    // 在宿主上扫出来的绝对路径在沙箱里并不存在。
    if crate::env::effective_env().kind() != crate::env::EnvKind::Local {
        return Some(PathBuf::from(name));
    }
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
        // headless/desktop 档位下 ffmpeg 在沙箱里，必须在环境里探测；
        // 只有 local 档位才走宿主 `which`。
        let env = crate::env::effective_env();
        let (ok, stdout): (bool, Vec<u8>) = if env.kind() == crate::env::EnvKind::Local {
            let output = Command::new("which").arg(name).output().ok()?;
            (output.status.success(), output.stdout)
        } else {
            let out = env
                .exec(
                    &["which".to_string(), name.to_string()],
                    crate::env::ExecOptions::default(),
                )
                .ok()?;
            (out.status == 0, out.stdout)
        };
        if !ok {
            return None;
        }
        let text = String::from_utf8_lossy(&stdout);
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
