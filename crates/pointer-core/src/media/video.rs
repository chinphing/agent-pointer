use anyhow::{Context, Result};
use base64::Engine;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// OSS single PutObject limit (technical ceiling); Composer does not hard-reject below this.
pub const MAX_VIDEO_BYTES: usize = 5 * 1024 * 1024 * 1024;

/// Soft advisory — user confirms before code compress to this size, then OSS upload.
pub const COMPOSER_VIDEO_ADVISORY_BYTES: usize = 500 * 1024 * 1024;

/// Qwen/DashScope native `video_url` Base64 data-URL payload limit (encoded string, not raw file).
pub const MAX_VIDEO_API_BASE64_BYTES: usize = 10 * 1024 * 1024;

/// Max raw video bytes so `data:video/…;base64,…` stays under [`MAX_VIDEO_API_BASE64_BYTES`].
pub fn max_raw_bytes_for_native_video_api(mime: &str) -> usize {
    let mime = mime.trim();
    let mime = if mime.is_empty() { "video/mp4" } else { mime };
    let prefix_len = format!("data:{mime};base64,").len();
    let b64_budget = MAX_VIDEO_API_BASE64_BYTES.saturating_sub(prefix_len + 64);
    b64_budget * 3 / 4
}

/// Encoded length of a `video_url` data URL for a raw payload of `raw_len` bytes.
pub fn video_data_url_encoded_len(mime: &str, raw_len: usize) -> usize {
    let mime = mime.trim();
    let mime = if mime.is_empty() { "video/mp4" } else { mime };
    format!("data:{mime};base64,").len() + (raw_len + 2) / 3 * 4
}

/// Download video bytes from an OSS/public HTTPS URL (ffmpeg fallback path).
pub async fn download_video_from_url(url: &str, cancel: &CancellationToken) -> Result<Vec<u8>> {
    let url = url.trim();
    if url.is_empty() {
        anyhow::bail!("empty video URL");
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .context("video download http client")?;
    let resp = tokio::select! {
        _ = cancel.cancelled() => anyhow::bail!("cancelled"),
        r = client.get(url).send() => r?,
    };
    if !resp.status().is_success() {
        anyhow::bail!("video download HTTP {}", resp.status());
    }
    let bytes = tokio::select! {
        _ = cancel.cancelled() => anyhow::bail!("cancelled"),
        r = resp.bytes() => r?,
    };
    Ok(bytes.to_vec())
}

/// Default frames per second when the user does not set `framesPerSecond`.
pub const DEFAULT_FRAMES_PER_SECOND: f64 = 1.0;
/// Max frames sent to the vision model per `media_understand` call.
pub const MAX_VISION_FRAMES_PER_CALL: usize = 200;

/// Resolved time window and sampling for one video understanding call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoTimeRange {
    pub start_sec: f64,
    pub end_sec: f64,
    pub frames_per_second: f64,
    pub user_specified_time: bool,
}

impl VideoTimeRange {
    pub fn window_sec(&self) -> f64 {
        (self.end_sec - self.start_sec).max(0.0)
    }

    pub fn frame_count(&self) -> usize {
        let window = self.window_sec();
        if window <= 0.0 {
            return 1;
        }
        ((window * self.frames_per_second).ceil() as usize).max(1)
    }

    pub fn max_window_sec_for_fps(fps: f64) -> f64 {
        MAX_VISION_FRAMES_PER_CALL as f64 / fps.max(f64::MIN_POSITIVE)
    }

    pub fn normalize(duration_sec: f64, start: f64, end: f64, fps: f64) -> Result<Self> {
        if !start.is_finite() || !end.is_finite() || start < 0.0 {
            anyhow::bail!("timeStartSec must be a finite number >= 0");
        }
        if !end.is_finite() || end < 0.0 {
            anyhow::bail!("timeEndSec must be a finite number >= 0");
        }
        if start > end {
            anyhow::bail!("timeStartSec ({start}) must be <= timeEndSec ({end})");
        }
        let duration = duration_sec.max(0.0);
        if end > duration {
            anyhow::bail!("timeEndSec ({end}) exceeds video duration ({duration:.1}s)");
        }
        Ok(Self {
            start_sec: start,
            end_sec: end,
            frames_per_second: fps,
            user_specified_time: true,
        })
    }

    pub fn default_first_window(duration_sec: f64, fps: f64) -> Result<Self> {
        let duration = duration_sec.max(0.0);
        let end = duration.min(Self::max_window_sec_for_fps(fps));
        Ok(Self {
            start_sec: 0.0,
            end_sec: end,
            frames_per_second: fps,
            user_specified_time: false,
        })
    }

    pub fn ensure_within_per_call_limit(&self) -> Result<()> {
        let count = self.frame_count();
        if count > MAX_VISION_FRAMES_PER_CALL {
            anyhow::bail!(
                "requested {count} frames ({:.1}s–{:.1}s at {} frame(s)/second); max {MAX_VISION_FRAMES_PER_CALL} per call",
                self.start_sec,
                self.end_sec,
                self.frames_per_second
            );
        }
        Ok(())
    }
}

pub fn format_video_scope_notice(
    range: &VideoTimeRange,
    total_duration_sec: f64,
    frame_count: usize,
    native_video: bool,
    native_via_oss: bool,
) -> String {
    let scope = format!("{:.1}s–{:.1}s", range.start_sec, range.end_sec);
    let total = total_duration_sec.max(0.0);
    let fps = range.frames_per_second;
    let sampling = if native_video {
        if native_via_oss {
            format!("native video_url (OSS URL) at {fps} frame(s)/second (server-side sampling)")
        } else {
            format!("native video_url at {fps} frame(s)/second (server-side sampling)")
        }
    } else {
        format!("{frame_count} ffmpeg frame(s) at {fps} frame(s)/second")
    };
    if range.user_specified_time {
        format!("[Video scope: {scope} of {total:.1}s total — extracted as requested; {sampling}.]")
    } else {
        format!(
            "[Video scope: {scope} of {total:.1}s total — only the first segment was processed ({sampling}). For other segments, describe the window in **goal** and call again, or split into batches of at most {MAX_VISION_FRAMES_PER_CALL} frames per call.]"
        )
    }
}

/// Probe video duration via ffprobe (writes a temp copy of `bytes`).
pub fn probe_video_duration(bytes: &[u8], file_name: &str) -> Result<f64> {
    let ffprobe = crate::media::ffmpeg::resolve_ffprobe().context("ffprobe not found")?;
    let suffix = video_suffix(file_name);
    let mut input = tempfile::Builder::new()
        .prefix("pointer-vid-probe-")
        .suffix(suffix)
        .tempfile()
        .context("video probe temp input")?;
    input
        .write_all(bytes)
        .context("write video probe temp input")?;
    probe_duration(&ffprobe, input.path())
}

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
        Ok(out) if out.status.success() => match std::fs::read(&output_path) {
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
        },
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

/// True when `file_name` looks like a common video container.
pub fn is_video_file_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    [
        ".mp4", ".m4v", ".mov", ".webm", ".mkv", ".mpeg", ".mpg", ".avi",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// Steps applied when shrinking an oversized video.
#[derive(Debug, Clone, Default)]
pub struct VideoShrinkReport {
    pub original_bytes: usize,
    pub final_bytes: usize,
    pub applied: Vec<&'static str>,
}

impl VideoShrinkReport {
    pub fn changed(&self) -> bool {
        !self.applied.is_empty() && self.final_bytes != self.original_bytes
    }
}

pub fn format_video_shrink_notice(report: &VideoShrinkReport) -> Option<String> {
    if report.applied.is_empty() {
        return None;
    }
    Some(format!(
        "[Video size: {:.1} MB → {:.1} MB via {}.]",
        report.original_bytes as f64 / (1024.0 * 1024.0),
        report.final_bytes as f64 / (1024.0 * 1024.0),
        report.applied.join(", ")
    ))
}

/// Reduce video size when above `max_bytes`: faststart → compress → lower resolution.
pub fn shrink_video_to_max(
    bytes: &[u8],
    file_name: &str,
    max_bytes: usize,
) -> Result<(Vec<u8>, VideoShrinkReport)> {
    let original = bytes.len();
    if original <= max_bytes {
        return Ok((
            bytes.to_vec(),
            VideoShrinkReport {
                original_bytes: original,
                final_bytes: original,
                applied: Vec::new(),
            },
        ));
    }

    let ffmpeg =
        crate::media::ffmpeg::resolve_ffmpeg().context("ffmpeg not found for video shrink")?;
    let mut current = bytes.to_vec();
    let mut applied = Vec::new();

    log::info!("video {file_name}: {original} bytes exceeds {max_bytes} limit; attempting shrink");

    current = remux_video_faststart(&current, file_name);
    applied.push("faststart");
    log::info!(
        "video {file_name}: faststart remux → {} bytes",
        current.len()
    );
    if current.len() <= max_bytes {
        let final_bytes = current.len();
        return Ok((
            current,
            VideoShrinkReport {
                original_bytes: original,
                final_bytes,
                applied,
            },
        ));
    }

    let attempts: [(&str, &[&str]); 6] = [
        (
            "compress",
            &[
                "-c:v", "libx264", "-preset", "fast", "-crf", "28", "-c:a", "aac", "-b:a", "128k",
            ],
        ),
        (
            "compress+scale1280",
            &[
                "-vf",
                "scale='min(1280,iw)':-2",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "30",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
            ],
        ),
        (
            "compress+scale960",
            &[
                "-vf",
                "scale='min(960,iw)':-2",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "32",
                "-c:a",
                "aac",
                "-b:a",
                "96k",
            ],
        ),
        (
            "compress+scale720",
            &[
                "-vf",
                "scale='min(720,iw)':-2",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "35",
                "-c:a",
                "aac",
                "-b:a",
                "64k",
            ],
        ),
        (
            "compress+scale480",
            &[
                "-vf",
                "scale='min(480,iw)':-2",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "38",
                "-an",
            ],
        ),
        (
            "compress+scale360",
            &[
                "-vf",
                "scale='min(360,iw)':-2",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "40",
                "-an",
            ],
        ),
    ];

    for (label, extra) in attempts {
        if current.len() <= max_bytes {
            break;
        }
        match ffmpeg_transcode_bytes(&ffmpeg, &current, file_name, extra) {
            Ok(next) if !next.is_empty() && next.len() < current.len() => {
                log::info!(
                    "video {file_name}: {label} {} → {} bytes",
                    current.len(),
                    next.len()
                );
                current = next;
                applied.push(label);
            }
            Ok(next) if !next.is_empty() => {
                log::warn!(
                    "video {file_name}: {label} did not reduce size ({} → {} bytes)",
                    current.len(),
                    next.len()
                );
            }
            Ok(_) => log::warn!("video {file_name}: {label} produced empty output"),
            Err(e) => log::warn!("video {file_name}: {label} failed: {e:#}"),
        }
    }

    if current.len() > max_bytes {
        anyhow::bail!(
            "video {file_name} is {:.1} MB after shrink attempts; limit is {:.1} MB",
            current.len() as f64 / (1024.0 * 1024.0),
            max_bytes as f64 / (1024.0 * 1024.0)
        );
    }

    let final_bytes = current.len();
    Ok((
        current,
        VideoShrinkReport {
            original_bytes: original,
            final_bytes,
            applied,
        },
    ))
}

fn ffmpeg_transcode_bytes(
    ffmpeg: &PathBuf,
    bytes: &[u8],
    file_name: &str,
    extra_args: &[&str],
) -> Result<Vec<u8>> {
    let suffix = video_suffix(file_name);
    let mut input = tempfile::Builder::new()
        .prefix("pointer-vid-shrink-in-")
        .suffix(suffix)
        .tempfile()
        .context("video shrink temp input")?;
    input.write_all(bytes).context("write video shrink input")?;
    let input_path = input.path();

    let output = tempfile::Builder::new()
        .prefix("pointer-vid-shrink-out-")
        .suffix(".mp4")
        .tempfile()
        .context("video shrink temp output")?;
    let out_path = output.path();

    let mut args = vec![
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-i",
        input_path.to_str().unwrap_or_default(),
    ];
    args.extend_from_slice(extra_args);
    args.extend_from_slice(&[
        "-movflags",
        "+faststart",
        "-y",
        out_path.to_str().unwrap_or_default(),
    ]);

    let out = Command::new(ffmpeg)
        .args(&args)
        .output()
        .context("ffmpeg video shrink")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!("ffmpeg shrink failed: {stderr}");
    }
    let bytes = std::fs::read(out_path).context("read shrunk video")?;
    Ok(bytes)
}

/// DashScope `fps` parameter valid range for native `video_url` input.
pub const DASHSCOPE_VIDEO_FPS_MIN: f64 = 0.1;
pub const DASHSCOPE_VIDEO_FPS_MAX: f64 = 10.0;

pub fn dashscope_clamp_fps(fps: f64) -> f64 {
    if !fps.is_finite() || fps <= 0.0 {
        DEFAULT_FRAMES_PER_SECOND
    } else {
        fps.clamp(DASHSCOPE_VIDEO_FPS_MIN, DASHSCOPE_VIDEO_FPS_MAX)
    }
}

/// Returns a clip for `[range.start_sec, range.end_sec]` when the window is shorter than the full file.
pub fn prepare_video_bytes_for_range(
    bytes: &[u8],
    file_name: &str,
    range: &VideoTimeRange,
    duration_sec: f64,
) -> Result<Vec<u8>> {
    let duration = duration_sec.max(0.0);
    let needs_clip = range.start_sec > f64::EPSILON || range.end_sec < duration - f64::EPSILON;
    if !needs_clip {
        return Ok(bytes.to_vec());
    }
    extract_video_clip_bytes(bytes, file_name, range.start_sec, range.window_sec())
}

/// Extract a time segment with ffmpeg (`-c copy` first, re-encode on failure).
pub fn extract_video_clip_bytes(
    bytes: &[u8],
    file_name: &str,
    start_sec: f64,
    duration_sec: f64,
) -> Result<Vec<u8>> {
    let ffmpeg =
        crate::media::ffmpeg::resolve_ffmpeg().context("ffmpeg not found for video clip")?;
    let suffix = video_suffix(file_name);
    let mut input = tempfile::Builder::new()
        .prefix("pointer-vid-clip-in-")
        .suffix(suffix)
        .tempfile()
        .context("video clip temp input")?;
    input
        .write_all(bytes)
        .context("write video clip temp input")?;
    let input_path = input.path();

    let output = tempfile::Builder::new()
        .prefix("pointer-vid-clip-out-")
        .suffix(suffix)
        .tempfile()
        .context("video clip temp output")?;
    let out_path = output.path();

    let start = start_sec.max(0.0);
    let duration = duration_sec.max(0.1);
    let start_s = format!("{start:.3}");
    let duration_s = format!("{duration:.3}");
    let input_s = input_path.to_str().unwrap_or_default();
    let output_s = out_path.to_str().unwrap_or_default();

    let copy_out = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-ss",
            &start_s,
            "-i",
            input_s,
            "-t",
            &duration_s,
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-y",
            output_s,
        ])
        .output();
    if let Ok(out) = copy_out {
        if out.status.success() {
            if let Ok(clipped) = std::fs::read(out_path) {
                if !clipped.is_empty() {
                    log::info!(
                        "video clip ok (copy): {file_name} start={start:.1}s duration={duration:.1}s → {} bytes",
                        clipped.len()
                    );
                    return Ok(clipped);
                }
            }
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            log::warn!("video clip copy failed for {file_name}: {stderr}");
        }
    }

    let reencode_out = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-ss",
            &start_s,
            "-i",
            input_s,
            "-t",
            &duration_s,
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
            "-y",
            output_s,
        ])
        .output()
        .context("ffmpeg clip reencode")?;
    if !reencode_out.status.success() {
        let stderr = String::from_utf8_lossy(&reencode_out.stderr)
            .trim()
            .to_string();
        anyhow::bail!("ffmpeg clip reencode failed for {file_name}: {stderr}");
    }
    let clipped = std::fs::read(out_path).context("read clipped video")?;
    if clipped.is_empty() {
        anyhow::bail!("ffmpeg clip produced empty output for {file_name}");
    }
    log::info!(
        "video clip ok (reencode): {file_name} start={start:.1}s duration={duration:.1}s → {} bytes",
        clipped.len()
    );
    Ok(clipped)
}

pub fn video_mime_from_file_name(file_name: &str) -> &'static str {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".mov") {
        "video/quicktime"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else {
        "video/mp4"
    }
}

pub fn extract_video_frame_base64s(bytes: &[u8], file_name: &str) -> Result<Vec<String>> {
    let duration = probe_video_duration(bytes, file_name)?;
    let range = VideoTimeRange::default_first_window(duration, DEFAULT_FRAMES_PER_SECOND)?;
    extract_video_frame_base64s_with_range(bytes, file_name, &range).map(|(frames, _)| frames)
}

pub fn extract_video_frame_base64s_with_range(
    bytes: &[u8],
    file_name: &str,
    range: &VideoTimeRange,
) -> Result<(Vec<String>, VideoExtractMeta)> {
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
    let start = range.start_sec.clamp(0.0, duration.max(0.0));
    let end = range.end_sec.clamp(start, duration.max(start));
    let fps = range.frames_per_second.max(f64::MIN_POSITIVE);
    let frame_count = range.frame_count();

    let mut frames = Vec::new();
    for i in 0..frame_count {
        let t = if frame_count <= 1 {
            start
        } else {
            start + (i as f64) / fps
        };
        if t > end + f64::EPSILON {
            break;
        }
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
        frames.push(base64::engine::general_purpose::STANDARD.encode(&frame_bytes));
    }
    if frames.is_empty() {
        anyhow::bail!(
            "ffmpeg extracted no frames from {file_name} (codec/format may be unsupported or file damaged)"
        );
    }
    let extracted = frames.len();
    Ok((
        frames,
        VideoExtractMeta {
            duration_sec: duration,
            frame_count: extracted,
        },
    ))
}

/// Metadata returned alongside extracted frames for user-facing scope notices.
#[derive(Debug, Clone, Copy)]
pub struct VideoExtractMeta {
    pub duration_sec: f64,
    pub frame_count: usize,
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
    text.trim().parse::<f64>().context("parse ffprobe duration")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_scope_notice_default_vs_user() {
        let default = VideoTimeRange {
            start_sec: 0.0,
            end_sec: 200.0,
            frames_per_second: 1.0,
            user_specified_time: false,
        };
        let notice = format_video_scope_notice(&default, 500.0, 200, true, false);
        assert!(notice.contains("did not specify a time window"));
        assert!(notice.contains("native video_url"));

        let user = VideoTimeRange {
            start_sec: 10.0,
            end_sec: 20.0,
            frames_per_second: 2.0,
            user_specified_time: true,
        };
        let notice = format_video_scope_notice(&user, 120.0, 20, false, false);
        assert!(notice.contains("as requested"));
        assert!(notice.contains("ffmpeg frame"));
    }

    #[test]
    fn video_time_range_rejects_too_many_frames() {
        let range = VideoTimeRange {
            start_sec: 0.0,
            end_sec: 300.0,
            frames_per_second: 1.0,
            user_specified_time: true,
        };
        assert!(range.ensure_within_per_call_limit().is_err());
    }

    #[test]
    fn video_shrink_notice_formats_applied_steps() {
        let report = VideoShrinkReport {
            original_bytes: 150 * 1024 * 1024,
            final_bytes: 80 * 1024 * 1024,
            applied: vec!["faststart", "compress"],
        };
        let notice = format_video_shrink_notice(&report).unwrap();
        assert!(notice.contains("faststart"));
        assert!(notice.contains("150.0 MB"));
    }

    #[test]
    fn max_raw_bytes_for_native_video_api_fits_base64_limit() {
        let mime = "video/mp4";
        let max_raw = max_raw_bytes_for_native_video_api(mime);
        assert!(max_raw > 0);
        assert!(video_data_url_encoded_len(mime, max_raw) < MAX_VIDEO_API_BASE64_BYTES);
        assert!(video_data_url_encoded_len(mime, max_raw + 1024) >= MAX_VIDEO_API_BASE64_BYTES);
    }

    #[test]
    fn remux_video_faststart_produces_valid_mp4_when_ffmpeg_available() {
        let Some(ffmpeg) = crate::media::ffmpeg::resolve_ffmpeg() else {
            return;
        };

        let input_path =
            std::env::temp_dir().join(format!("pointer_remux_test_in_{}.mp4", std::process::id()));
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
