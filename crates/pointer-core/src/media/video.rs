use anyhow::{Context, Result};
use base64::Engine;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_FRAMES: usize = 5;

/// Remux video with `-movflags +faststart` to move the moov atom to the beginning of the file.
///
/// IM platforms (Feishu, DingTalk, etc.) need the moov atom at the start to display
/// correct duration metadata in message previews. AI-generated MP4s often have the
/// moov atom at the end, causing the platform to show "0s".
///
/// Returns the remuxed bytes on success, or the **original bytes** if ffmpeg is
/// unavailable or the operation fails (best-effort, never blocks delivery).
pub fn remux_video_faststart(bytes: &[u8], file_name: &str) -> Vec<u8> {
    let ffmpeg = match crate::media::ffmpeg::resolve_ffmpeg() {
        Some(p) => p,
        None => return bytes.to_vec(),
    };

    let suffix = video_suffix(file_name);
    let mut input = match tempfile::Builder::new()
        .prefix("pointer-vid-remux-")
        .suffix(suffix)
        .tempfile()
    {
        Ok(f) => f,
        Err(e) => {
            log::warn!("video faststart: tempfile create failed: {e}");
            return bytes.to_vec();
        }
    };
    if let Err(e) = input.write_all(bytes) {
        log::warn!("video faststart: tempfile write failed: {e}");
        return bytes.to_vec();
    }
    if let Err(e) = input.flush() {
        log::warn!("video faststart: tempfile flush failed: {e}");
        return bytes.to_vec();
    }
    // Close the handle before ffmpeg reads (required on Windows).
    let input_path = input.into_temp_path();

    let output_dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("video faststart: output tempdir failed: {e}");
            return bytes.to_vec();
        }
    };
    let output_path = output_dir.path().join(format!("out{suffix}"));

    let result = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-i",
            input_path.to_str().unwrap_or_default(),
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-y",
            output_path.to_str().unwrap_or_default(),
        ])
        .output();

    match result {
        Ok(out) if out.status.success() => {
            match std::fs::read(&output_path) {
                Ok(remuxed) if !remuxed.is_empty() => {
                    log::info!(
                        "video faststart ok: {} bytes → {} bytes for {}",
                        bytes.len(),
                        remuxed.len(),
                        file_name
                    );
                    return remuxed;
                }
                Ok(_) => log::warn!("video faststart: output empty for {file_name}"),
                Err(e) => log::warn!("video faststart: read output failed: {e}"),
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            log::warn!("video faststart failed for {file_name}: {stderr}");
        }
        Err(e) => {
            log::warn!("video faststart spawn failed: {e}");
        }
    }
    bytes.to_vec()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remux_video_faststart_produces_valid_mp4_when_ffmpeg_available() {
        let Some(ffmpeg) = crate::media::ffmpeg::resolve_ffmpeg() else {
            return;
        };

        let input_path = std::env::temp_dir().join(format!(
            "pointer_remux_test_in_{}.mp4",
            std::process::id()
        ));
        let status = std::process::Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:d=1",
                "-c:v",
                "libx264",
                "-y",
                input_path.to_str().unwrap_or_default(),
            ])
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success(), "ffmpeg test input generation failed");

        let bytes = std::fs::read(&input_path).expect("read test input");
        let remuxed = remux_video_faststart(&bytes, "test.mp4");
        assert!(!remuxed.is_empty());
        assert!(remuxed.windows(4).any(|w| w == b"ftyp"));
        let _ = std::fs::remove_file(input_path);
    }
}
