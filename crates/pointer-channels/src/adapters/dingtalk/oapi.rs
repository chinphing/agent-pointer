//! DingTalk oapi helpers (media upload + sessionWebhook replies).

use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::traits::OutboundMedia;

const OAPI_FILE_EXTENSIONS: &[&str] = &[
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "zip", "pdf", "rar",
];

/// DingTalk oapi often returns HTTP 200 with `errcode != 0`.
pub fn check_oapi_errcode(resp: &Value, step: &str) -> Result<()> {
    let Some(errcode) = resp.get("errcode").and_then(|v| v.as_i64()) else {
        return Ok(());
    };
    if errcode == 0 {
        return Ok(());
    }
    let errmsg = resp
        .get("errmsg")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown error");
    anyhow::bail!("dingtalk oapi {step} failed: {errmsg} (errcode={errcode})");
}

/// New OpenAPI (`api.dingtalk.com`) may return `{ success, code, message }` instead of errcode.
pub fn check_openapi_response(resp: &Value, step: &str) -> Result<()> {
    if let Some(success) = resp.get("success").and_then(|v| v.as_bool()) {
        if !success {
            let message = resp
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            anyhow::bail!("dingtalk openapi {step} failed: {message}");
        }
    }
    if let Some(code) = resp.get("code").and_then(|v| v.as_str()) {
        let ok = code.is_empty() || code == "0" || code.eq_ignore_ascii_case("OK");
        if !ok {
            let message = resp
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            anyhow::bail!("dingtalk openapi {step} failed: {message} (code={code})");
        }
    }
    check_oapi_errcode(resp, step)
}

/// `sampleFile` template `fileType` values (robot batchSend / groupMessages).
pub fn sample_file_type(file_name: &str) -> &'static str {
    match oapi_file_extension(file_name).as_deref() {
        Some("pdf") => "pdf",
        Some("zip") => "zip",
        Some("rar") => "rar",
        Some("doc") => "doc",
        Some("docx") => "docx",
        Some("xlsx") => "xlsx",
        _ => "zip",
    }
}

pub fn is_group_conversation_key(conversation_key: &str) -> bool {
    conversation_key.contains(":group:")
}

pub fn oapi_file_extension(file_name: &str) -> Option<String> {
    let name = file_name.trim();
    let ext = name.rsplit('.').next()?;
    if ext == name || ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

pub fn oapi_supports_file_name(file_name: &str) -> bool {
    oapi_file_extension(file_name)
        .map(|ext| OAPI_FILE_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(false)
}

pub struct PreparedOapiUpload {
    pub bytes: Vec<u8>,
    pub file_name: String,
    pub zipped: bool,
}

/// Prepare bytes for `media/upload` `type=file`. Unsupported extensions (e.g. `.html`) are
/// wrapped in a zip archive because oapi only accepts office/archive types.
pub fn prepare_oapi_file_upload(media: &OutboundMedia) -> Result<PreparedOapiUpload> {
    if media.is_image() {
        return Ok(PreparedOapiUpload {
            bytes: media.bytes.clone(),
            file_name: media.file_name.clone(),
            zipped: false,
        });
    }
    if oapi_supports_file_name(&media.file_name) {
        return Ok(PreparedOapiUpload {
            bytes: media.bytes.clone(),
            file_name: media.file_name.clone(),
            zipped: false,
        });
    }
    let inner = media.file_name.trim();
    let inner = if inner.is_empty() { "attachment.bin" } else { inner };
    let zip_name = format!(
        "{}.zip",
        inner
            .rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(inner)
    );
    let bytes = zip_single_file(inner, &media.bytes)
        .with_context(|| format!("zip outbound file for dingtalk upload: {inner}"))?;
    log::info!(
        "dingtalk oapi: wrapped unsupported file {inner} as {zip_name} ({} bytes)",
        bytes.len()
    );
    Ok(PreparedOapiUpload {
        bytes,
        file_name: zip_name,
        zipped: true,
    })
}

fn zip_single_file(entry_name: &str, data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(entry_name, options)
            .context("zip start_file")?;
        zip.write_all(data).context("zip write_all")?;
        zip.finish().context("zip finish")?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_oapi_errcode_ok() {
        let v = serde_json::json!({"errcode": 0, "errmsg": "ok"});
        assert!(check_oapi_errcode(&v, "test").is_ok());
    }

    #[test]
    fn check_oapi_errcode_fails() {
        let v = serde_json::json!({"errcode": 40004, "errmsg": "invalid media type"});
        let err = check_oapi_errcode(&v, "upload").unwrap_err();
        assert!(err.to_string().contains("40004"));
    }

    #[test]
    fn html_is_wrapped_as_zip() {
        let media = OutboundMedia {
            file_name: "minesweeper.html".into(),
            mime_type: "text/html".into(),
            bytes: b"<html></html>".to_vec(),
            local_path: None,
        };
        let prepared = prepare_oapi_file_upload(&media).unwrap();
        assert!(prepared.zipped);
        assert_eq!(prepared.file_name, "minesweeper.zip");
        assert!(prepared.bytes.starts_with(b"PK"));
    }

    #[test]
    fn pdf_is_not_zipped() {
        let media = OutboundMedia {
            file_name: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            bytes: b"%PDF".to_vec(),
            local_path: None,
        };
        let prepared = prepare_oapi_file_upload(&media).unwrap();
        assert!(!prepared.zipped);
        assert_eq!(prepared.file_name, "report.pdf");
    }

    #[test]
    fn sample_file_type_maps_extensions() {
        assert_eq!(sample_file_type("empty.zip"), "zip");
        assert_eq!(sample_file_type("report.pdf"), "pdf");
        assert_eq!(sample_file_type("data.xlsx"), "xlsx");
    }
}
