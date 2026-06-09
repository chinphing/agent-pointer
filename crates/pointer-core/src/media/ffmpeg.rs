use std::path::PathBuf;
use std::process::Command;

/// Returns true when both ffmpeg and ffprobe resolve on this machine.
pub fn ffmpeg_available() -> bool {
    resolve_tool("ffmpeg").is_some() && resolve_tool("ffprobe").is_some()
}

pub fn resolve_ffmpeg() -> Option<PathBuf> {
    resolve_tool("ffmpeg")
}

pub fn resolve_ffprobe() -> Option<PathBuf> {
    resolve_tool("ffprobe")
}

fn resolve_tool(name: &str) -> Option<PathBuf> {
    if let Some(path) = resolve_via_which(name) {
        return Some(path);
    }
    for dir in extra_search_dirs() {
        let candidate = dir.join(tool_filename(name));
        if candidate.is_file() {
            return Some(candidate);
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

fn resolve_via_which(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let output = Command::new("where").arg(name).output().ok()?;
    #[cfg(not(windows))]
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
