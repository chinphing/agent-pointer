use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type HmacSha256 = Hmac<Sha256>;

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feishu_signature_matches_known_format() {
        let sig = feishu_webhook_signature("123", "nonce", "key", "{}");
        assert!(!sig.is_empty());
        assert_eq!(sig.len(), 64);
    }
}
