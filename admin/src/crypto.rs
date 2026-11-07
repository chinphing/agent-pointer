//! 企业 Key 的本地加密（AES-256-GCM，密钥材料由本机标识派生）。
//!
//! 与 `pointer-core` 的 `local_secret` 同一算法与标签体系，但**独立实现**：
//! admin 不依赖 pointer-core，避免把整个客户端依赖树拖进控制面进程。
//!
//! 后果要记住：加密材料绑定本机——**换机、迁移、从备份恢复到新机器后 Key 需重新录入**。
//! 数据库放在远端（MySQL/PG）不受影响：密钥在进程侧，从不进库。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use base64::Engine;
use hkdf::Hkdf;
use sha2::Sha256;

const PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;
const APP_PEPPER: &[u8] = b"pointer-admin-secret-v1";
const INFO_PROVIDER_KEY: &[u8] = b"admin-provider-keys-v1";

/// 已加密的密钥。字段私有，只能由 [`encrypt_secret`] 或 [`EncryptedSecret::from_stored`] 构造，
/// 防止把明文当密文写进库。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret(String);

impl EncryptedSecret {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 从库里的 `enc:v1:…` 字符串还原。
    pub fn from_stored(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if !raw.starts_with(PREFIX) {
            return Err(anyhow!("stored secret is not in enc:v1 format"));
        }
        Ok(Self(raw.to_string()))
    }
}

fn machine_identity() -> Vec<u8> {
    match machine_uid::get() {
        Ok(id) if !id.trim().is_empty() => id.into_bytes(),
        _ => {
            let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown-host".into());
            let user = std::env::var("USER").unwrap_or_else(|_| "unknown-user".into());
            format!("{host}:{user}").into_bytes()
        }
    }
}

fn derive_key(info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(APP_PEPPER), &machine_identity());
    let mut key = [0u8; 32];
    hk.expand(info, &mut key).expect("hkdf expand to 32 bytes");
    key
}

fn cipher() -> Result<Aes256Gcm> {
    let key = derive_key(INFO_PROVIDER_KEY);
    Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow!("AES key init: {e}"))
}

pub fn encrypt_secret(plaintext: &str) -> Result<EncryptedSecret> {
    if plaintext.trim().is_empty() {
        return Err(anyhow!("secret must not be empty"));
    }
    use rand::RngCore;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher()?
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("encrypt secret: {e}"))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    let encoded = base64::engine::general_purpose::STANDARD.encode(blob);
    Ok(EncryptedSecret(format!("{PREFIX}{encoded}")))
}

pub fn decrypt_secret(secret: &EncryptedSecret) -> Result<String> {
    let body = secret
        .0
        .strip_prefix(PREFIX)
        .ok_or_else(|| anyhow!("stored secret is not in enc:v1 format"))?;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| anyhow!("decode secret: {e}"))?;
    if blob.len() <= NONCE_LEN {
        return Err(anyhow!("secret blob too short"));
    }
    let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
    let plain = cipher()?
        .decrypt(nonce, &blob[NONCE_LEN..])
        .map_err(|_| anyhow!("decrypt secret failed (tampered or wrong machine)"))?;
    String::from_utf8(plain).map_err(|e| anyhow!("secret is not utf-8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let secret = encrypt_secret("sk-test-123").expect("encrypt");
        assert!(secret.as_str().starts_with("enc:v1:"));
        assert!(!secret.as_str().contains("sk-test-123"));
        assert_eq!(decrypt_secret(&secret).expect("decrypt"), "sk-test-123");
    }

    #[test]
    fn same_plaintext_encrypts_differently() {
        let a = encrypt_secret("sk-same").expect("a");
        let b = encrypt_secret("sk-same").expect("b");
        assert_ne!(a, b, "random nonce must produce different ciphertext");
        assert_eq!(decrypt_secret(&a).expect("a"), decrypt_secret(&b).expect("b"));
    }

    #[test]
    fn tampered_secret_fails_to_decrypt() {
        let secret = encrypt_secret("sk-test").expect("encrypt");
        let mut raw = secret.as_str().to_string();
        // 改掉密文最后一个字符
        let last = raw.pop().expect("non-empty");
        raw.push(if last == 'A' { 'B' } else { 'A' });
        let tampered = EncryptedSecret::from_stored(&raw).expect("format ok");
        assert!(decrypt_secret(&tampered).is_err());
    }

    #[test]
    fn plaintext_is_rejected_by_from_stored() {
        assert!(EncryptedSecret::from_stored("sk-plain-text").is_err());
    }

    #[test]
    fn empty_secret_rejected() {
        assert!(encrypt_secret("").is_err());
        assert!(encrypt_secret("   ").is_err());
    }
}
