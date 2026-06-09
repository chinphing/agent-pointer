use aes::cipher::{block_padding::NoPadding, block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use aes::Aes256;
use anyhow::{anyhow, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD as B64, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// Decrypt Feishu encrypted event/callback body (`base64(iv + ciphertext)`).
pub fn feishu_decrypt_payload(encrypt_key: &str, encrypted_b64: &str) -> Result<String> {
    let key = Sha256::digest(encrypt_key.as_bytes());
    let raw = B64.decode(encrypted_b64).context("feishu encrypt b64")?;
    if raw.len() <= 16 {
        return Err(anyhow!("feishu encrypt payload too short"));
    }
    let (iv, cipher_bytes) = raw.split_at(16);
    let cipher = Aes256CbcDec::new_from_slices(&key, iv).context("feishu aes256 cbc")?;
    let mut buf = cipher_bytes.to_vec();
    let plain = cipher
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("feishu decrypt: {e}"))?;
    String::from_utf8(plain.to_vec()).context("feishu decrypt utf8")
}

/// Parse webhook JSON; decrypt when `encrypt` field is present.
pub fn feishu_decode_event_body(raw_body: &str, encrypt_key: &str) -> Result<serde_json::Value> {
    let outer: serde_json::Value = serde_json::from_str(raw_body).context("feishu json")?;
    if let Some(enc) = outer.get("encrypt").and_then(|v| v.as_str()) {
        let key = encrypt_key.trim();
        if key.is_empty() {
            return Err(anyhow!("feishu encrypt_key required for encrypted payload"));
        }
        let plain = feishu_decrypt_payload(key, enc)?;
        return serde_json::from_str(&plain).context("feishu decrypted json");
    }
    Ok(outer)
}

pub fn feishu_webhook_signature(
    timestamp: &str,
    nonce: &str,
    encrypt_key: &str,
    raw_body: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{timestamp}{nonce}{encrypt_key}{raw_body}"));
    format!("{:x}", hasher.finalize())
}

pub fn dingtalk_sign(timestamp: &str, secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(timestamp.as_bytes());
    B64.encode(mac.finalize().into_bytes())
}

pub fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// WeCom/WXBizMsgCrypt compatible AES-CBC decrypt for callback messages.
pub fn wecom_decrypt(
    encoding_aes_key: &str,
    corp_id: &str,
    encrypted: &str,
) -> Result<String> {
    let key = B64.decode(format!("{encoding_aes_key}=")).context("aes key b64")?;
    if key.len() != 32 {
        return Err(anyhow!("invalid aes key length"));
    }
    let cipher_bytes = B64.decode(encrypted).context("cipher b64")?;
    let iv = &key[..16];
    let cipher = Aes128CbcDec::new_from_slices(&key[..16], iv).context("cbc dec")?;
    let mut buf = cipher_bytes.to_vec();
    let plain = cipher
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("decrypt: {e}"))?;
    if plain.len() < 20 {
        return Err(anyhow!("plaintext too short"));
    }
    let content_len = u32::from_be_bytes(plain[16..20].try_into().unwrap()) as usize;
    let end = 20 + content_len;
    if end > plain.len() {
        return Err(anyhow!("invalid content length"));
    }
    let msg = std::str::from_utf8(&plain[20..end]).context("utf8 msg")?;
    let from_corp = std::str::from_utf8(&plain[end..]).unwrap_or("");
    if !corp_id.is_empty() && !from_corp.is_empty() && from_corp != corp_id {
        log::warn!("wecom decrypt corp mismatch expected={corp_id} got={from_corp}");
    }
    Ok(msg.to_string())
}

pub fn wecom_encrypt(
    encoding_aes_key: &str,
    corp_id: &str,
    plaintext: &str,
) -> Result<String> {
    let key = B64.decode(format!("{encoding_aes_key}=")).context("aes key b64")?;
    let iv = &key[..16];
    let mut msg = rand::random::<[u8; 16]>().to_vec();
    msg.extend_from_slice(&(plaintext.len() as u32).to_be_bytes());
    msg.extend_from_slice(plaintext.as_bytes());
    msg.extend_from_slice(corp_id.as_bytes());
    let cipher = Aes128CbcEnc::new_from_slices(&key[..16], iv).context("cbc enc")?;
    let msg_len = msg.len();
    let mut buf = msg;
    let enc = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, msg_len)
        .map_err(|e| anyhow!("encrypt: {e}"))?;
    Ok(B64.encode(enc))
}

pub fn wecom_msg_signature(token: &str, timestamp: &str, nonce: &str, encrypt: &str) -> String {
    let mut parts = vec![token.to_string(), timestamp.to_string(), nonce.to_string()];
    if !encrypt.is_empty() {
        parts.push(encrypt.to_string());
    }
    parts.sort();
    sha1_hex(parts.join("").as_bytes())
}

fn pad_b64_input(s: &str) -> String {
    let rem = s.len() % 4;
    if rem == 0 {
        return s.to_string();
    }
    let mut out = s.to_string();
    out.extend(std::iter::repeat_n('=', 4 - rem));
    out
}

fn decode_hex_bytes(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        anyhow::bail!("hex length must be even");
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks(2) {
        let s = std::str::from_utf8(chunk).context("hex byte")?;
        out.push(u8::from_str_radix(s, 16).context("hex byte")?);
    }
    Ok(out)
}

fn try_b64_decode_loose(s: &str) -> Option<Vec<u8>> {
    let padded = pad_b64_input(s);
    B64.decode(&padded)
        .ok()
        .or_else(|| STANDARD_NO_PAD.decode(s).ok())
        .or_else(|| URL_SAFE.decode(pad_b64_input(s)).ok())
        .or_else(|| URL_SAFE_NO_PAD.decode(s).ok())
}

/// Decode per-message WeCom AI bot `aeskey` (Base64 w/o padding, URL-safe, or 64-char hex).
pub fn decode_wecom_aibot_aes_key(raw: &str) -> Result<Vec<u8>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty wecom aibot aeskey");
    }
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return decode_hex_bytes(trimmed);
    }
    if let Some(decoded) = try_b64_decode_loose(trimmed) {
        if decoded.len() >= 32 {
            return Ok(decoded);
        }
        if let Ok(text) = std::str::from_utf8(&decoded) {
            let inner = text.trim();
            if inner.len() == 64 && inner.chars().all(|c| c.is_ascii_hexdigit()) {
                return decode_hex_bytes(inner);
            }
        }
        if !decoded.is_empty() {
            return Ok(decoded);
        }
    }
    anyhow::bail!(
        "unsupported wecom aibot aeskey format (len={})",
        trimmed.len()
    )
}

fn strip_wecom_pkcs7_padding(decrypted: &[u8]) -> Result<Vec<u8>> {
    if decrypted.is_empty() {
        return Err(anyhow!("wecom aibot decrypt: empty plaintext"));
    }
    let pad_len = decrypted[decrypted.len() - 1] as usize;
    if pad_len < 1 || pad_len > 32 || pad_len > decrypted.len() {
        return Err(anyhow!("wecom aibot invalid PKCS#7 padding value"));
    }
    if !decrypted[decrypted.len() - pad_len..]
        .iter()
        .all(|&b| b as usize == pad_len)
    {
        return Err(anyhow!("wecom aibot PKCS#7 padding bytes mismatch"));
    }
    Ok(decrypted[..decrypted.len() - pad_len].to_vec())
}

/// Decrypt WeCom AI bot media using per-message `aeskey` (AES-256-CBC, IV=key[0..16]).
/// PKCS#7 padding may span up to 32 bytes per WeCom AI bot docs (see official aibot-node-sdk).
pub fn wecom_aibot_decrypt_file(encrypted: &[u8], aes_key_raw: &str) -> Result<Vec<u8>> {
    if encrypted.is_empty() {
        return Err(anyhow!("wecom aibot decrypt: empty payload"));
    }
    let key = decode_wecom_aibot_aes_key(aes_key_raw).context("wecom aibot aeskey decode")?;
    if key.len() < 32 {
        return Err(anyhow!(
            "wecom aibot aeskey too short ({} bytes)",
            key.len()
        ));
    }
    let iv = &key[..16];
    let cipher = Aes256CbcDec::new_from_slices(&key, iv).context("wecom aibot aes256 cbc")?;
    let mut buf = encrypted.to_vec();
    if buf.len() % 16 != 0 {
        return Err(anyhow!(
            "wecom aibot ciphertext length {} not block-aligned",
            buf.len()
        ));
    }
    let decrypted = cipher
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| anyhow!("wecom aibot aes decrypt: {e}"))?;
    strip_wecom_pkcs7_padding(decrypted)
}

/// AES-128-ECB + PKCS7 for Weixin iLink CDN upload (reference weixin-ilink cdn.py).
pub fn aes128_ecb_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyInit};
    type EcbEnc = ecb::Encryptor<aes::Aes128>;
    let cipher = EcbEnc::new_from_slice(key).context("weixin ecb key")?;
    let mut buf = data.to_vec();
    let padded_len = ((data.len() + 15) / 16) * 16 + 16;
    buf.resize(padded_len, 0);
    let enc = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .map_err(|e| anyhow!("weixin ecb encrypt: {e}"))?;
    Ok(enc.to_vec())
}

pub fn md5_hex(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feishu_signature_matches_known_format() {
        let sig = feishu_webhook_signature("123", "nonce", "key", "{}");
        assert!(!sig.is_empty());
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn wecom_aibot_aeskey_decode_without_padding() {
        let key = [0xABu8; 32];
        let b64_no_pad = STANDARD_NO_PAD.encode(key);
        let decoded = decode_wecom_aibot_aes_key(&b64_no_pad).expect("decode");
        assert_eq!(decoded, key);
    }

    #[test]
    fn wecom_aibot_aeskey_decode_hex() {
        let key = decode_wecom_aibot_aes_key("00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff")
            .expect("hex");
        assert_eq!(key.len(), 32);
        assert_eq!(key[0], 0x00);
    }

    #[test]
    fn wecom_aibot_decrypt_roundtrip_pkcs7_32() {
        type Aes256CbcEnc = cbc::Encryptor<Aes256>;
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        let key_b64 = STANDARD_NO_PAD.encode(key);
        let iv = &key[..16];
        let plain = b"wecom test image";
        let block = 32usize;
        let pad_len = block - (plain.len() % block);
        let mut padded = plain.to_vec();
        padded.extend(std::iter::repeat_n(pad_len as u8, pad_len));
        let enc_cipher = Aes256CbcEnc::new_from_slices(&key, iv).expect("enc init");
        let plain_len = padded.len();
        let mut enc_buf = vec![0u8; plain_len + 32];
        enc_buf[..plain_len].copy_from_slice(&padded);
        enc_cipher
            .encrypt_padded_mut::<NoPadding>(&mut enc_buf, plain_len)
            .expect("encrypt");
        enc_buf.truncate(plain_len);
        let out = wecom_aibot_decrypt_file(&enc_buf, &key_b64).expect("decrypt");
        assert_eq!(out, plain);
    }

    #[test]
    fn feishu_decrypt_roundtrip() {
        use aes::Aes256;
        use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
        type Aes256CbcEnc = cbc::Encryptor<Aes256>;

        let encrypt_key = "test_encrypt_key";
        let payload = r#"{"type":"url_verification","challenge":"abc123"}"#;
        let key = Sha256::digest(encrypt_key.as_bytes());
        let iv: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let cipher = Aes256CbcEnc::new_from_slices(&key, &iv).expect("enc");
        let mut buf = vec![0u8; payload.len() + 32];
        buf[..payload.len()].copy_from_slice(payload.as_bytes());
        let enc = cipher
            .encrypt_padded_mut::<Pkcs7>(&mut buf, payload.len())
            .expect("pad");
        let mut wire = iv.to_vec();
        wire.extend_from_slice(enc);
        let encrypted_b64 = B64.encode(wire);
        let plain =
            feishu_decrypt_payload(encrypt_key, &encrypted_b64).expect("decrypt");
        assert!(plain.contains("abc123"));
    }
}
