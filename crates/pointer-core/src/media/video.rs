use anyhow::{Context, Result};
use base64::Engine;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_FRAMES: usize = 5;

pub fn extract_video_frame_base64s(bytes: &[u8], file_name: &str) -> Result<Vec<String>> {
    let ffmpeg = crate::media::ffmpeg::resolve_ffmpeg().context("ffmpeg not found")?;
    let ffprobe = crate::media::ffmpeg::resolve_ffprobe().context("ffprobe not found")?;

    let suffix = video_suffix(file_name);
    let mut input = tempfile::Builder::new()
        .prefix("pointer-vid-")
        .suffix(suffix)
        .tempfile()
        .context("video temp input")?;
    input.write_all(bytes).context("write video temp input")?;
    let input_path = input.path();

    let duration = probe_duration(&ffprobe, input_path).unwrap_or(0.0);
    let frame_count = if duration <= 0.0 {
        1
    } else {
        MAX_FRAMES
    };

    let mut frames = Vec::new();
    for i in 0..frame_count {
        let t = if frame_count <= 1 {
            0.0
        } else {
            duration * (i as f64) / (frame_count - 1) as f64
        };
        let output = tempfile::Builder::new()
            .suffix(".jpg")
            .tempfile()
            .context("video temp frame")?;
        let out_path = output.path();
        let output = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-ss",
                &format!("{t:.3}"),
                "-i",
                input_path.to_str().unwrap_or_default(),
                "-vframes",
                "1",
                "-q:v",
                "3",
                "-y",
                out_path.to_str().unwrap_or_default(),
            ])
            .output()
            .with_context(|| format!("ffmpeg frame extract at t={t}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            log::warn!(
                "ffmpeg frame extract failed at t={t} for {file_name}: {}",
                if stderr.is_empty() {
                    "no stderr".into()
                } else {
                    stderr.clone()
                }
            );
            continue;
        }
        let frame_bytes = std::fs::read(out_path).context("read ffmpeg frame")?;
        if frame_bytes.is_empty() {
            continue;
        }
        frames.push(
            base64::engine::general_purpose::STANDARD.encode(&frame_bytes),
        );
    }
    if frames.is_empty() {
        anyhow::bail!(
            "ffmpeg extracted no frames from {file_name} (codec/format may be unsupported or file damaged)"
        );
    }
    Ok(frames)
}

fn video_suffix(file_name: &str) -> &'static str {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".mov") {
        ".mov"
    } else if lower.ends_with(".webm") {
        ".webm"
    } else {
        ".mp4"
    }
}

fn probe_duration(ffprobe: &PathBuf, path: &Path) -> Result<f64> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path.to_str().unwrap_or_default(),
        ])
        .output()
        .context("ffprobe spawn")?;
    if !output.status.success() {
        anyhow::bail!("ffprobe failed");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse::<f64>()
        .context("parse ffprobe duration")
}
