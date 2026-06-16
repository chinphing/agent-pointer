use aes::Aes128;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use cipher::{block_padding::NoPadding, BlockDecryptMut, KeyInit};
use ecb::Decryptor;

use crate::http_client::HttpClient;

pub const WEIXIN_CDN_BASE: &str = "https://novac2c.cdn.weixin.qq.com/c2c";

type Aes128EcbDec = Decryptor<Aes128>;

pub fn decode_weixin_aes_key(raw: &str) -> Result<[u8; 16]> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty weixin aes key");
    }
    if trimmed.len() == 32 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut key = [0u8; 16];
        for (i, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
            if i >= 16 {
                break;
            }
            let s = std::str::from_utf8(chunk).context("hex key")?;
            key[i] = u8::from_str_radix(s, 16).context("hex key byte")?;
        }
        return Ok(key);
    }
    let decoded = B64.decode(trimmed).context("weixin aes key b64")?;
    if decoded.len() == 16 {
        let mut key = [0u8; 16];
        key.copy_from_slice(&decoded);
        return Ok(key);
    }
    if decoded.len() == 32 {
        let text = std::str::from_utf8(&decoded).context("weixin aes key ascii hex")?;
        if text.chars().all(|c| c.is_ascii_hexdigit()) {
            return decode_weixin_aes_key(text);
        }
    }
    anyhow::bail!("unsupported weixin aes key format (len={})", decoded.len())
}

pub fn decrypt_weixin_cdn_bytes(ciphertext: &[u8], aes_key_raw: &str) -> Result<Vec<u8>> {
    if ciphertext.is_empty() {
        return Ok(Vec::new());
    }
    let key = decode_weixin_aes_key(aes_key_raw)?;
    let cipher = Aes128EcbDec::new_from_slice(&key).context("weixin aes128 ecb init")?;
    let mut buf = ciphertext.to_vec();
    let len = buf.len();
    if len % 16 != 0 {
        anyhow::bail!("weixin ciphertext length {len} is not a multiple of 16");
    }
    let plain = cipher
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| anyhow!("weixin aes decrypt: {e}"))?;
    Ok(plain.to_vec())
}

pub async fn download_cdn_bytes(http: &HttpClient, encrypt_query_param: &str) -> Result<Vec<u8>> {
    let param = encrypt_query_param.trim();
    if param.is_empty() {
        anyhow::bail!("weixin cdn missing encrypt_query_param");
    }
    let url = format!(
        "{WEIXIN_CDN_BASE}/download?encrypted_query_param={}",
        urlencoding::encode(param)
    );
    let (bytes, _, _) = http.get_bytes(&url, &[]).await?;
    Ok(bytes)
}

pub async fn download_and_decrypt(
    http: &HttpClient,
    encrypt_query_param: &str,
    aes_key: Option<&str>,
    image_aeskey_hex: Option<&str>,
) -> Result<Vec<u8>> {
    let encrypted = download_cdn_bytes(http, encrypt_query_param).await?;
    let key = image_aeskey_hex
        .filter(|s| !s.trim().is_empty())
        .or(aes_key.filter(|s| !s.trim().is_empty()));
    let Some(key_raw) = key else {
        log::info!("weixin cdn download without aes key; using plaintext bytes");
        return Ok(encrypted);
    };
    decrypt_weixin_cdn_bytes(&encrypted, key_raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex_aes_key() {
        let key = decode_weixin_aes_key("00112233445566778899aabbccddeeff").unwrap();
        assert_eq!(key[0], 0x00);
        assert_eq!(key[15], 0xff);
    }

    #[test]
    fn decode_b64_raw_key() {
        let key = decode_weixin_aes_key("ABEiM0RVZneImaq7zN3u/w==").unwrap();
        assert_eq!(key.len(), 16);
    }

    #[test]
    fn decode_b64_hex_string_key() {
        let key = decode_weixin_aes_key("MDAxMTIyMzM0NDU1NjY3Nzg4OTlhYWJiY2NkZGVlZmY=").unwrap();
        assert_eq!(key[0], 0x00);
        assert_eq!(key[15], 0xff);
    }
}
