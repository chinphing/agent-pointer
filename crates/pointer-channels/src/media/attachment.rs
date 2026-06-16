use base64::Engine;
use pointer_core::models::MediaAttachment;

use pointer_core::media::normalize_inbound_filename;

use crate::adapters::feishu::auth::{guess_mime_from_name, kind_from_mime};

pub struct DownloadedMedia {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub file_name: String,
}

pub fn to_media_attachment(downloaded: DownloadedMedia, kind_hint: &str) -> MediaAttachment {
    let kind = kind_from_mime(&downloaded.mime_type, kind_hint);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&downloaded.bytes);
    MediaAttachment {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        mime_type: downloaded.mime_type,
        file_name: downloaded.file_name,
        size_bytes: downloaded.bytes.len() as u64,
        storage_rel_path: None,
        content_base64: Some(b64),
        derived_text: None,
        local_abs_path: None,
    }
}

/// Sniff common image/audio/video types from magic bytes (IM CDN often returns octet-stream).
pub fn guess_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("image/jpeg");
    }
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 12 && bytes[4..8] == *b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"heic" || brand == b"heix" || brand == b"mif1" {
            return Some("image/heic");
        }
    }
    if bytes.len() >= 4 && &bytes[0..4] == b"%PDF" {
        return Some("application/pdf");
    }
    if bytes.len() >= 4 && bytes[0..2] == *b"PK" {
        if zip_contains_entry(bytes, "word/document.xml") {
            return Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        }
        if zip_contains_entry(bytes, "xl/workbook.xml") {
            return Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
        }
        if zip_contains_entry(bytes, "ppt/presentation.xml") {
            return Some("application/vnd.openxmlformats-officedocument.presentationml.presentation");
        }
    }
    if bytes.len() >= 12
        && (bytes[4..8] == *b"ftyp"
            && (bytes[8..12] == *b"isom" || bytes[8..12] == *b"mp41" || bytes[8..12] == *b"avc1"))
    {
        return Some("video/mp4");
    }
    if bytes.len() >= 4 && bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WAVE" {
        return Some("audio/wav");
    }
    None
}

fn extension_for_mime(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "application/pdf" => "pdf",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "video/mp4" => "mp4",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/opus" => "ogg",
        "audio/amr" => "amr",
        _ => "bin",
    }
}

fn replace_bin_extension(file_name: &str, ext: &str) -> String {
    if file_name.to_ascii_lowercase().ends_with(".bin") {
        let stem = file_name
            .trim_end_matches(".bin")
            .trim_end_matches(".BIN");
        return format!("{stem}.{ext}");
    }
    format!("{file_name}.{ext}")
}

pub fn finalize_downloaded(
    mut downloaded: DownloadedMedia,
    file_name: Option<String>,
    kind_hint: Option<&str>,
) -> DownloadedMedia {
    if let Some(name) = file_name.filter(|s| !s.trim().is_empty()) {
        downloaded.file_name = normalize_inbound_filename(name.trim());
    }
    downloaded.file_name = normalize_inbound_filename(&downloaded.file_name);
    if downloaded.mime_type == "application/octet-stream" {
        if let Some(m) = guess_mime_from_bytes(&downloaded.bytes) {
            downloaded.mime_type = m.into();
        }
    }
    if downloaded.mime_type == "application/octet-stream" {
        downloaded.mime_type = guess_mime_from_name(&downloaded.file_name);
    }
    let ext = extension_for_mime(&downloaded.mime_type);
    if ext != "bin" && downloaded.file_name.to_ascii_lowercase().ends_with(".bin") {
        downloaded.file_name = replace_bin_extension(&downloaded.file_name, ext);
    } else if ext == "bin" {
        if let Some(hint) = kind_hint {
            let hinted = match hint {
                "image" => Some("image/jpeg".to_string()),
                "video" => Some("video/mp4".to_string()),
                "audio" => Some("audio/mpeg".to_string()),
                "document" => {
                    let from_name = guess_mime_from_name(&downloaded.file_name);
                    if from_name != "application/octet-stream" {
                        Some(from_name)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(hinted) = hinted {
                downloaded.mime_type = hinted;
                let hinted_ext = extension_for_mime(&downloaded.mime_type);
                if downloaded.file_name.to_ascii_lowercase().ends_with(".bin") {
                    downloaded.file_name =
                        replace_bin_extension(&downloaded.file_name, hinted_ext);
                }
            }
        }
    }
    downloaded
}

fn zip_contains_entry(bytes: &[u8], entry: &str) -> bool {
    use std::io::Cursor;
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return false;
    };
    let found = zip.by_name(entry).is_ok();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_jpeg_from_magic() {
        let bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(guess_mime_from_bytes(&bytes), Some("image/jpeg"));
    }

    #[test]
    fn sniff_ogg_from_magic() {
        let bytes = b"OggS\x00\x02\x00\x00";
        assert_eq!(guess_mime_from_bytes(bytes), Some("audio/ogg"));
    }

    #[test]
    fn finalize_wecom_image_bin_to_jpg() {
        let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
        let out = finalize_downloaded(
            DownloadedMedia {
                bytes,
                mime_type: "application/octet-stream".into(),
                file_name: "wecom-test.bin".into(),
            },
            None,
            Some("image"),
        );
        assert_eq!(out.mime_type, "image/jpeg");
        assert!(out.file_name.ends_with(".jpg"));
    }

    #[test]
    fn finalize_wecom_docx_bin_renamed() {
        use std::io::Cursor;
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let mut buf = Vec::new();
        {
            let mut zip = ZipWriter::new(Cursor::new(&mut buf));
            zip.start_file(
                "word/document.xml",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(
                br#"<w:document><w:body><w:p><w:r><w:t>Hello docx</w:t></w:r></w:p></w:body></w:document>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let out = finalize_downloaded(
            DownloadedMedia {
                bytes: buf,
                mime_type: "application/octet-stream".into(),
                file_name: "wecom-test.bin".into(),
            },
            None,
            Some("document"),
        );
        assert_eq!(
            out.mime_type,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert!(out.file_name.ends_with(".docx"));
    }
}

pub const CHANNEL_MEDIA_MAX_BYTES: usize = 30 * 1024 * 1024;

pub fn enforce_max_bytes(bytes: &[u8], label: &str) -> anyhow::Result<()> {
    if bytes.len() > CHANNEL_MEDIA_MAX_BYTES {
        anyhow::bail!(
            "{label} exceeds {} MB",
            CHANNEL_MEDIA_MAX_BYTES / (1024 * 1024)
        );
    }
    Ok(())
}
