use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::RngCore;
use serde_json::{json, Value};

use super::cdn::WEIXIN_CDN_BASE;
use super::ilink_client::ILinkClient;
use crate::crypto::{aes128_ecb_encrypt, md5_hex};

const MEDIA_IMAGE: u64 = 1;
const MEDIA_FILE: u64 = 3;

pub struct UploadedWeixinMedia {
    pub encrypt_query_param: String,
    pub aes_key_b64: String,
    pub aes_key_hex: String,
    pub file_size: u64,
    pub file_md5: String,
}

fn aes_padded_size(raw_len: usize) -> usize {
    let block = 16;
    raw_len + (block - (raw_len % block))
}

pub async fn upload_weixin_media(
    client: &ILinkClient,
    to_user_id: &str,
    file_name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<UploadedWeixinMedia> {
    let media_type = if mime_type.starts_with("image/") {
        MEDIA_IMAGE
    } else {
        MEDIA_FILE
    };
    let label = if media_type == MEDIA_IMAGE {
        "image"
    } else {
        "file"
    };

    let mut aes_key = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);
    let aes_key_hex = bytes_to_hex(&aes_key);
    let aes_key_b64 = B64.encode(aes_key);

    let mut filekey = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut filekey);
    let filekey_hex = bytes_to_hex(&filekey);

    let rawsize = bytes.len() as u64;
    let filesize = aes_padded_size(bytes.len()) as u64;
    let rawfilemd5 = md5_hex(bytes);

    let upload_resp = client
        .get_upload_url(
            to_user_id,
            &filekey_hex,
            media_type,
            rawsize,
            filesize,
            &rawfilemd5,
            &aes_key_hex,
        )
        .await
        .with_context(|| format!("weixin getuploadurl {label}"))?;
    let upload_param = upload_resp
        .get("upload_param")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .context("weixin upload_param missing")?;

    let ciphertext = aes128_ecb_encrypt(&aes_key, bytes)?;
    let upload_url = format!(
        "{WEIXIN_CDN_BASE}/upload?encrypted_query_param={}&filekey={}",
        urlencoding::encode(upload_param),
        urlencoding::encode(&filekey_hex)
    );
    let (_resp, headers) = client
        .post_cdn_bytes(&upload_url, &ciphertext)
        .await
        .with_context(|| format!("weixin cdn upload {label} {file_name}"))?;
    let download_param = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-encrypted-param"))
        .map(|(_, v)| v.clone())
        .context("weixin cdn x-encrypted-param missing")?;

    log::info!(
        "weixin cdn upload ok {label} file={file_name} rawsize={rawsize}"
    );

    Ok(UploadedWeixinMedia {
        encrypt_query_param: download_param,
        aes_key_b64: aes_key_b64,
        aes_key_hex,
        file_size: rawsize,
        file_md5: rawfilemd5,
    })
}

pub fn image_item_json(uploaded: &UploadedWeixinMedia) -> Value {
    json!({
        "type": 2,
        "image_item": {
            "media": {
                "encrypt_query_param": uploaded.encrypt_query_param,
                "aes_key": uploaded.aes_key_b64,
                "encrypt_type": 1
            },
            "aeskey": uploaded.aes_key_hex,
            "mid_size": uploaded.file_size
        }
    })
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn file_item_json(uploaded: &UploadedWeixinMedia, file_name: &str) -> Value {
    json!({
        "type": 4,
        "file_item": {
            "media": {
                "encrypt_query_param": uploaded.encrypt_query_param,
                "aes_key": B64.encode(uploaded.aes_key_hex.as_bytes()),
                "encrypt_type": 1
            },
            "file_name": file_name,
            "md5": uploaded.file_md5,
            "len": uploaded.file_size.to_string()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_item_matches_ilink_protocol() {
        let uploaded = UploadedWeixinMedia {
            encrypt_query_param: "AAFFc8c2PXQ5mKPw7rbcH7S1EA=".into(),
            aes_key_b64: String::new(),
            aes_key_hex: "00112233445566778899aabbccddeeff".into(),
            file_size: 14_529,
            file_md5: "9d2a7b9c3e2f1d41c7d5b3a1a7e1c6f0".into(),
        };
        let item = file_item_json(&uploaded, "minesweeper.html");
        let media = &item["file_item"]["media"];
        assert_eq!(media["encrypt_type"], 1);
        assert_eq!(
            media["aes_key"].as_str(),
            Some("MDAxMTIyMzM0NDU1NjY3Nzg4OTlhYWJiY2NkZGVlZmY=")
        );
        assert_eq!(item["file_item"]["md5"], "9d2a7b9c3e2f1d41c7d5b3a1a7e1c6f0");
        assert_eq!(item["file_item"]["len"], "14529");
        assert_eq!(item["file_item"]["file_name"], "minesweeper.html");
    }
}
