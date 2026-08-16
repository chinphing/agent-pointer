//! Local AES-256-GCM encryption for OAuth refresh tokens (`auth.dat`).
//!
//! Key material is derived per-machine via HKDF; no OS keyring prompts.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use hkdf::Hkdf;
use sha2::Sha256;

const FILE_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const APP_PEPPER: &[u8] = b"pointer-app-local-secret-v1";

fn machine_identity() -> Vec<u8> {
    match machine_uid::get() {
        Ok(id) if !id.trim().is_empty() => id.into_bytes(),
        Ok(_) => {
            log::warn!("local_secret: empty machine id; falling back to hostname+username");
            fallback_identity()
        }
        Err(e) => {
            log::warn!(
                "local_secret: machine-uid unavailable ({e}); falling back to hostname+username"
            );
            fallback_identity()
        }
    }
}

fn fallback_identity() -> Vec<u8> {
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown-host".into());
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".into());
    format!("{host}:{user}").into_bytes()
}

fn derive_key_with_info(info: &[u8]) -> [u8; 32] {
    let identity = machine_identity();
    let hk = Hkdf::<Sha256>::new(Some(APP_PEPPER), &identity);
    let mut key = [0u8; 32];
    hk.expand(info, &mut key).expect("HKDF expand to 32 bytes");
    key
}

/// HMAC key for public media download tokens (separate from auth.dat key).
pub fn derive_media_download_key() -> Result<[u8; 32]> {
    Ok(derive_key_with_info(b"media-public-download-v1"))
}

/// Encrypt plaintext for local storage. Output: `version(1B)` + `nonce(12B)` + ciphertext+tag.
pub fn encrypt_local_secret(plaintext: &str) -> Result<Vec<u8>> {
    encrypt_local_secret_with_info(plaintext, b"auth-refresh-token")
}

/// Encrypt with a purpose-specific derived key (e.g. user provider keys),
/// isolated from the `auth.dat` key so files are not interchangeable.
pub fn encrypt_local_secret_with_info(plaintext: &str, info: &[u8]) -> Result<Vec<u8>> {
    let key = derive_key_with_info(info);
    let cipher = Aes256Gcm::new_from_slice(&key).context("AES key init")?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("encrypt failed: {e}"))?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    out.push(FILE_VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt blob written by [`encrypt_local_secret`].
pub fn decrypt_local_secret(blob: &[u8]) -> Result<String> {
    decrypt_local_secret_with_info(blob, b"auth-refresh-token")
}

/// Decrypt with the matching purpose-specific derived key.
pub fn decrypt_local_secret_with_info(blob: &[u8], info: &[u8]) -> Result<String> {
    if blob.is_empty() {
        return Err(anyhow!("empty auth blob"));
    }
    if blob[0] != FILE_VERSION {
        return Err(anyhow!("unsupported auth blob version {}", blob[0]));
    }
    if blob.len() < 1 + NONCE_LEN + 16 {
        return Err(anyhow!("auth blob too short"));
    }
    let nonce = Nonce::from_slice(&blob[1..1 + NONCE_LEN]);
    let ciphertext = &blob[1 + NONCE_LEN..];
    let key = derive_key_with_info(info);
    let cipher = Aes256Gcm::new_from_slice(&key).context("AES key init")?;
    let plain = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("decrypt failed (tampered or wrong machine): {e}"))?;
    String::from_utf8(plain).context("auth plaintext not utf-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let blob = encrypt_local_secret("refresh-token-abc").unwrap();
        assert_eq!(blob[0], FILE_VERSION);
        let plain = decrypt_local_secret(&blob).unwrap();
        assert_eq!(plain, "refresh-token-abc");
    }

    #[test]
    fn encrypt_decrypt_roundtrip_with_info() {
        let blob =
            encrypt_local_secret_with_info("sk-user-typed", b"provider-api-keys-v1").unwrap();
        let plain = decrypt_local_secret_with_info(&blob, b"provider-api-keys-v1").unwrap();
        assert_eq!(plain, "sk-user-typed");
        // Purpose-scoped key isolation: cannot decrypt with the auth.dat info.
        assert!(decrypt_local_secret_with_info(&blob, b"auth-refresh-token").is_err());
    }

    #[test]
    fn tampered_blob_fails() {
        let mut blob = encrypt_local_secret("secret").unwrap();
        if let Some(b) = blob.last_mut() {
            *b ^= 0xff;
        }
        assert!(decrypt_local_secret(&blob).is_err());
    }
}
