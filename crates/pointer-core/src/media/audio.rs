//! Normalize IM / channel audio (e.g. Feishu `.bin`) for speech-to-text APIs.

use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

use super::video::is_video_file_name;

pub struct PreparedAudio {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub file_name: String,
    pub wire_format: String,
}

/// Optional conversation-media location for reading/writing a persisted `.wav` sibling.
pub struct AudioStorageContext {
    pub storage_rel_path: Option<String>,
    pub conversation_id: String,
    pub attachment_id: String,
}

/// Feishu voice and similar channels often ship opaque `.bin` blobs (opus etc.).
pub fn needs_audio_transcode(mime_type: &str, file_name: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    let lower = file_name.trim().to_ascii_lowercase();
    if lower.ends_with(".bin") {
        return true;
    }
    if mime.is_empty() || mime == "application/octet-stream" {
        return true;
    }
    if mime == "audio/opus" || mime == "audio/amr" || mime == "audio/silk" {
        return true;
    }
    false
}

fn audio_input_suffix(file_name: &str) -> &'static str {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        return ".mp3";
    }
    if lower.ends_with(".wav") {
        return ".wav";
    }
    if lower.ends_with(".m4a") {
        return ".m4a";
    }
    if lower.ends_with(".ogg") {
        return ".ogg";
    }
    if lower.ends_with(".amr") {
        return ".amr";
    }
    ".bin"
}

fn wav_output_name(file_name: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file_name)
        .trim();
    if stem.is_empty() {
        "audio.wav".into()
    } else {
        format!("{stem}.wav")
    }
}

/// True when bytes should be treated as a video container for ASR (extract audio track first).
pub fn is_video_source_for_asr(mime_type: &str, file_name: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    mime.starts_with("video/") || is_video_file_name(file_name)
}

fn video_input_suffix(file_name: &str) -> &'static str {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".mov") {
        ".mov"
    } else if lower.ends_with(".webm") {
        ".webm"
    } else if lower.ends_with(".mkv") {
        ".mkv"
    } else if lower.ends_with(".avi") {
        ".avi"
    } else if lower.ends_with(".mpeg") || lower.ends_with(".mpg") {
        ".mpeg"
    } else {
        ".mp4"
    }
}

/// Extract the audio track from a video file to 16 kHz mono WAV for ASR.
pub fn extract_audio_from_video_to_wav(bytes: &[u8], file_name: &str) -> Result<Vec<u8>> {
    let ffmpeg = crate::media::ffmpeg::resolve_ffmpeg().context("ffmpeg not found")?;
    let suffix = video_input_suffix(file_name);
    let mut input = tempfile::Builder::new()
        .prefix("pointer-vid-aud-")
        .suffix(suffix)
        .tempfile()
        .context("video audio temp input")?;
    input.write_all(bytes).context("write video temp input")?;
    let input_path = input.path();

    let output = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile()
        .context("video audio temp output")?;
    let out_path = output.path();

    let output = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-i",
            input_path.to_str().unwrap_or_default(),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-y",
            out_path.to_str().unwrap_or_default(),
        ])
        .output()
        .context("ffmpeg extract video audio")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        anyhow::bail!(
            "ffmpeg extract audio from video failed for {file_name}: {}",
            if detail.is_empty() {
                "no stderr"
            } else {
                detail
            }
        );
    }

    let wav = std::fs::read(out_path).context("read extracted video audio wav")?;
    if wav.is_empty() {
        anyhow::bail!("ffmpeg produced empty wav from video {file_name}");
    }
    Ok(wav)
}

pub fn transcode_audio_to_wav(bytes: &[u8], file_name: &str) -> Result<Vec<u8>> {
    let ffmpeg = crate::media::ffmpeg::resolve_ffmpeg().context("ffmpeg not found")?;
    let suffix = audio_input_suffix(file_name);
    let mut input = tempfile::Builder::new()
        .prefix("pointer-aud-")
        .suffix(suffix)
        .tempfile()
        .context("audio temp input")?;
    input.write_all(bytes).context("write audio temp input")?;
    let input_path = input.path();

    let output = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile()
        .context("audio temp output")?;
    let out_path = output.path();

    let output = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-i",
            input_path.to_str().unwrap_or_default(),
            "-ac",
            "1",
            "-ar",
            "16000",
            "-y",
            out_path.to_str().unwrap_or_default(),
        ])
        .output()
        .context("ffmpeg audio transcode")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        anyhow::bail!(
            "ffmpeg audio transcode failed for {file_name}: {}",
            if detail.is_empty() {
                "no stderr"
            } else {
                detail
            }
        );
    }

    let wav = std::fs::read(out_path).context("read transcoded wav")?;
    if wav.is_empty() {
        anyhow::bail!("ffmpeg produced empty wav for {file_name}");
    }
    Ok(wav)
}

pub fn prepare_audio_bytes_for_asr(
    bytes: &[u8],
    mime_type: &str,
    file_name: &str,
) -> Result<PreparedAudio> {
    prepare_audio_bytes_for_asr_cached(bytes, mime_type, file_name, None)
}

pub fn prepare_audio_bytes_for_asr_cached(
    bytes: &[u8],
    mime_type: &str,
    file_name: &str,
    storage: Option<&AudioStorageContext>,
) -> Result<PreparedAudio> {
    if let Some(ctx) = storage {
        if let Some(rel) = ctx
            .storage_rel_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            if let Some(wav_rel) = crate::media::store::stored_wav_sibling_rel(rel) {
                log::info!(
                    "media: using persisted wav for {} (skip transcode)",
                    file_name
                );
                let wav_bytes = crate::media::store::read_media_bytes(&wav_rel)?;
                let wav_name = wav_rel.rsplit('/').next().unwrap_or("audio.wav");
                return Ok(PreparedAudio {
                    bytes: wav_bytes,
                    mime_type: "audio/wav".into(),
                    file_name: wav_name.into(),
                    wire_format: "wav".into(),
                });
            }
        }
    }

    if is_video_source_for_asr(mime_type, file_name) {
        if !crate::media::ffmpeg::ffmpeg_available() {
            anyhow::bail!(
                "video speech transcription requires ffmpeg to extract the audio track from {file_name}"
            );
        }
        log::info!(
            "media: extracting audio track from video {file_name} (mime={mime_type}) for ASR"
        );
        let wav = extract_audio_from_video_to_wav(bytes, file_name)?;
        let prepared = PreparedAudio {
            bytes: wav,
            mime_type: "audio/wav".into(),
            file_name: wav_output_name(file_name),
            wire_format: "wav".into(),
        };
        if let Some(ctx) = storage {
            persist_playable_wav(ctx, &prepared);
        }
        return Ok(prepared);
    }

    let needs_transcode =
        crate::media::ffmpeg::ffmpeg_available() && needs_audio_transcode(mime_type, file_name);
    if needs_transcode {
        log::info!(
            "media: transcoding audio {} (mime={}) via ffmpeg for ASR",
            file_name,
            mime_type
        );
        let wav = transcode_audio_to_wav(bytes, file_name)?;
        let prepared = PreparedAudio {
            bytes: wav,
            mime_type: "audio/wav".into(),
            file_name: wav_output_name(file_name),
            wire_format: "wav".into(),
        };
        if let Some(ctx) = storage {
            persist_playable_wav(ctx, &prepared);
        }
        return Ok(prepared);
    }

    let wire_format = audio_wire_format(mime_type, file_name).to_string();
    Ok(PreparedAudio {
        bytes: bytes.to_vec(),
        mime_type: mime_type.to_string(),
        file_name: file_name.to_string(),
        wire_format,
    })
}

fn persist_playable_wav(ctx: &AudioStorageContext, prepared: &PreparedAudio) {
    if prepared.mime_type != "audio/wav" {
        return;
    }
    if let Some(rel) = ctx.storage_rel_path.as_deref() {
        if rel.to_ascii_lowercase().ends_with(".wav") {
            return;
        }
        if crate::media::store::stored_wav_sibling_rel(rel).is_some() {
            return;
        }
    }
    let conv = if !ctx.conversation_id.is_empty() {
        ctx.conversation_id.clone()
    } else if let Some(rel) = ctx.storage_rel_path.as_deref() {
        crate::media::store::parse_conversation_media_ids(rel)
            .map(|(c, _)| c)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let id = if !ctx.attachment_id.is_empty() {
        ctx.attachment_id.clone()
    } else if let Some(rel) = ctx.storage_rel_path.as_deref() {
        crate::media::store::parse_conversation_media_ids(rel)
            .map(|(_, i)| i)
            .unwrap_or_default()
    } else {
        String::new()
    };
    if conv.is_empty() || id.is_empty() {
        return;
    }
    persist_playable_wav_ids(&conv, &id, prepared);
}

fn persist_playable_wav_ids(conv: &str, id: &str, prepared: &PreparedAudio) {
    match crate::media::store::save_attachment_bytes(conv, id, &prepared.bytes, &prepared.file_name)
    {
        Ok(rel) => {
            log::info!("media: persisted playable wav at {}", rel);
        }
        Err(e) => {
            log::warn!(
                "media: failed to persist playable wav for {}: {:#}",
                prepared.file_name,
                e
            );
        }
    }
}

pub fn audio_wire_format(mime_type: &str, file_name: &str) -> &'static str {
    let m = mime_type.trim().to_ascii_lowercase();
    if m.contains("wav") {
        return "wav";
    }
    if m.contains("mpeg") || m.contains("mp3") {
        return "mp3";
    }
    if m.contains("mp4") || m.contains("m4a") {
        return "mp4";
    }
    if m.contains("webm") {
        return "webm";
    }
    if m.contains("ogg") {
        return "ogg";
    }
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".wav") {
        return "wav";
    }
    if lower.ends_with(".mp3") {
        return "mp3";
    }
    if lower.ends_with(".m4a") {
        return "mp4";
    }
    if lower.ends_with(".webm") {
        return "webm";
    }
    if lower.ends_with(".ogg") {
        return "ogg";
    }
    "wav"
}

/// Data URL for DashScope `multimodal-generation` audio parts.
pub fn wire_audio_data_for_multimodal(wire_format: &str, raw_base64: &str) -> String {
    let fmt = wire_format.trim().to_ascii_lowercase();
    let mime = match fmt.as_str() {
        "wav" => "wav",
        "mp3" | "mpeg" => "mpeg",
        "mp4" | "m4a" => "mp4",
        "ogg" => "ogg",
        "webm" => "webm",
        other => other,
    };
    format!("data:audio/{mime};base64,{raw_base64}")
}

/// DashScope compatible Chat API expects `data:audio/...;base64,...` not raw base64.
pub fn wire_audio_data_for_provider(
    provider: &crate::models::ProviderConfig,
    wire_format: &str,
    raw_base64: &str,
) -> String {
    if crate::models::provider_uses_dashscope_compatible_api(provider) {
        let fmt = wire_format.trim().to_ascii_lowercase();
        let mime = match fmt.as_str() {
            "wav" => "wav",
            "mp3" | "mpeg" => "mpeg",
            "mp4" | "m4a" => "mp4",
            "ogg" => "ogg",
            "webm" => "webm",
            other => other,
        };
        return format!("data:audio/{mime};base64,{raw_base64}");
    }
    raw_base64.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feishu_bin_needs_transcode() {
        assert!(needs_audio_transcode(
            "application/octet-stream",
            "file_v3_abc.bin"
        ));
    }

    #[test]
    fn wav_skips_transcode() {
        assert!(!needs_audio_transcode("audio/wav", "clip.wav"));
    }

    #[test]
    fn video_mp4_is_audio_source_for_asr() {
        assert!(is_video_source_for_asr("video/mp4", "demo.mp4"));
        assert!(!is_video_source_for_asr("audio/wav", "clip.wav"));
    }

    #[test]
    fn dashscope_wire_uses_data_url() {
        let provider = crate::models::ProviderConfig {
            id: "qwen".into(),
            name: "Qwen".into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            api_key: String::new(),
            models: vec![],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: std::collections::HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            extra_body: None,
            source: None,
        };
        let wired = wire_audio_data_for_provider(&provider, "wav", "abc123");
        assert_eq!(wired, "data:audio/wav;base64,abc123");
    }

    #[test]
    fn openai_wire_keeps_raw_base64() {
        let provider = crate::models::ProviderConfig {
            id: "openai".into(),
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            models: vec![],
            reasoning_in_messages: None,
            temperature: None,
            max_tokens: None,
            model_configs: std::collections::HashMap::new(),
            enable_thinking: None,
            thinking_budget: None,
            reasoning_effort: None,
            extra_body: None,
            source: None,
        };
        let wired = wire_audio_data_for_provider(&provider, "wav", "abc123");
        assert_eq!(wired, "abc123");
    }
}
