//! Normalize IM channel audio at ingress (Feishu OGG, WeCom AMR, DingTalk voice, etc.).

use crate::media::attachment::{finalize_downloaded, DownloadedMedia};
use crate::traits::InboundMediaRef;

pub fn normalize_channel_audio_download(
    mut downloaded: DownloadedMedia,
    media_ref: &InboundMediaRef,
) -> DownloadedMedia {
    let kind_hint = media_ref.kind.as_str();
    if is_channel_audio(media_ref, &downloaded) {
        // 渠道入站媒体落在宿主（下载器在宿主侧落盘），这里没有会话上下文：
        // 显式按宿主环境做归一化，不靠 `effective_env()` 猜。
        let env = pointer_core::env::host_env();
        if pointer_core::media::ffmpeg::ffmpeg_available(&env) {
            match pointer_core::media::audio::prepare_audio_bytes_for_asr(
                &env,
                &downloaded.bytes,
                &downloaded.mime_type,
                &downloaded.file_name,
            ) {
                Ok(prepared) => {
                    log::info!(
                        "channel audio normalized {} -> {} (mime={}) kind={}",
                        downloaded.file_name,
                        prepared.file_name,
                        prepared.mime_type,
                        kind_hint
                    );
                    downloaded.bytes = prepared.bytes;
                    downloaded.mime_type = prepared.mime_type;
                    downloaded.file_name = prepared.file_name;
                }
                Err(e) => {
                    log::warn!(
                        "channel audio normalize failed for {}: {:#}",
                        downloaded.file_name,
                        e
                    );
                }
            }
        } else {
            log::warn!(
                "channel audio {} skipped normalize: ffmpeg not available",
                downloaded.file_name
            );
        }
    }
    finalize_downloaded(downloaded, media_ref.file_name.clone(), Some(kind_hint))
}

fn is_channel_audio(media_ref: &InboundMediaRef, downloaded: &DownloadedMedia) -> bool {
    if media_ref.kind == "audio" {
        return true;
    }
    let mime = downloaded.mime_type.trim().to_ascii_lowercase();
    if mime.starts_with("audio/") {
        return true;
    }
    pointer_core::media::audio::needs_audio_transcode(&downloaded.mime_type, &downloaded.file_name)
}
