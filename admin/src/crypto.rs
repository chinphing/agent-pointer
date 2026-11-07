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
/// 用显式主密钥加密的密文前缀（多实例部署）。
const PREFIX_MASTER: &str = "enc:v2:";
/// 多实例部署时用这个环境变量提供主密钥；不配则沿用本机标识派生（单机默认）。
/// 多实例部署时用这个环境变量提供主密钥；不配则沿用本机标识派生（单机默认）。
pub const MASTER_KEY_ENV: &str = "POINTER_ADMIN_MASTER_KEY";
/// 主密钥模式的 HKDF salt：与机器标识模式用不同 salt，
/// 即使两者材料碰巧相同，派生出的密钥也不一样。
const MASTER_SALT: &[u8] = b"pointer-admin-master-v1";
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

    /// 从库里的 `enc:v1:…` / `enc:v2:…` 字符串还原。
    pub fn from_stored(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if !raw.starts_with(PREFIX) && !raw.starts_with(PREFIX_MASTER) {
            return Err(anyhow!("stored secret is not in enc:v1/v2 format"));
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

/// 显式主密钥（多实例部署）。未配置时返回 `None`，走本机标识。
fn master_material() -> Option<Vec<u8>> {
    std::env::var(MASTER_KEY_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(String::into_bytes)
}

/// 按模式派生密钥：`master = true` 用主密钥，否则用本机标识。
fn derive_key(info: &[u8], master: bool) -> Result<[u8; 32]> {
    let (salt, material) = if master {
        let material = master_material().ok_or_else(|| {
            anyhow!("密文需要 {MASTER_KEY_ENV}，但当前进程没有配置（或为空）")
        })?;
        (MASTER_SALT, material)
    } else {
        (APP_PEPPER, machine_identity())
    };
    let hk = Hkdf::<Sha256>::new(Some(salt), &material);
    let mut key = [0u8; 32];
    hk.expand(info, &mut key)
        .map_err(|e| anyhow!("hkdf expand: {e}"))?;
    Ok(key)
}

fn cipher(master: bool) -> Result<Aes256Gcm> {
    let key = derive_key(INFO_PROVIDER_KEY, master)?;
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

    // 配了主密钥就用它：密文可跨机器解密（多实例部署的前提）。
    let master = master_material().is_some();
    let ciphertext = cipher(master)?
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("encrypt secret: {e}"))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    let encoded = base64::engine::general_purpose::STANDARD.encode(blob);
    let prefix = if master { PREFIX_MASTER } else { PREFIX };
    Ok(EncryptedSecret(format!("{prefix}{encoded}")))
}

pub fn decrypt_secret(secret: &EncryptedSecret) -> Result<String> {
    // 前缀决定用哪把钥匙：v2 = 主密钥，v1 = 本机标识。
    // 这样切换部署模式时老数据仍能在原机器上解，新数据才用主密钥。
    let (body, master) = if let Some(body) = secret.0.strip_prefix(PREFIX_MASTER) {
        (body, true)
    } else if let Some(body) = secret.0.strip_prefix(PREFIX) {
        (body, false)
    } else {
        return Err(anyhow!("stored secret is not in enc:v1/v2 format"));
    };
    let blob = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| anyhow!("decode secret: {e}"))?;
    if blob.len() <= NONCE_LEN {
        return Err(anyhow!("secret blob too short"));
    }
    let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
    let plain = cipher(master)?
        .decrypt(nonce, &blob[NONCE_LEN..])
        .map_err(|_| {
            if master {
                anyhow!("decrypt secret failed (tampered, or wrong {MASTER_KEY_ENV})")
            } else {
                anyhow!("decrypt secret failed (tampered or wrong machine)")
            }
        })?;
    String::from_utf8(plain).map_err(|e| anyhow!("secret is not utf-8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 环境变量是进程级的，这几个用例必须串行跑，否则互相污染。
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_master(value: Option<&str>) {
        match value {
            Some(key) => std::env::set_var(MASTER_KEY_ENV, key),
            None => std::env::remove_var(MASTER_KEY_ENV),
        }
    }

    /// RAII：断言失败时也能把环境变量还原，不污染其他用例。
    struct MasterEnv;

    impl MasterEnv {
        fn set(value: Option<&str>) -> Self {
            set_master(value);
            Self
        }
    }

    impl Drop for MasterEnv {
        fn drop(&mut self) {
            set_master(None);
        }
    }

    #[test]
    fn roundtrip() {
        let _guard = lock();
        let _env = MasterEnv::set(None);
        let secret = encrypt_secret("sk-test-123").expect("encrypt");
        assert!(secret.as_str().starts_with("enc:v1:"));
        assert!(!secret.as_str().contains("sk-test-123"));
        assert_eq!(decrypt_secret(&secret).expect("decrypt"), "sk-test-123");
    }

    #[test]
    fn master_key_produces_v2_and_roundtrips() {
        let _guard = lock();
        let _env = MasterEnv::set(Some("unit-test-master-key-0123456789"));
        let secret = encrypt_secret("sk-master").expect("encrypt");
        assert!(
            secret.as_str().starts_with("enc:v2:"),
            "配了主密钥就该用 v2：{}",
            secret.as_str()
        );
        assert_eq!(decrypt_secret(&secret).expect("decrypt"), "sk-master");
    }

    #[test]
    fn v2_secret_needs_master_key_to_decrypt() {
        let _guard = lock();
        let _env = MasterEnv::set(Some("unit-test-master-key-0123456789"));
        let secret = encrypt_secret("sk-master").expect("encrypt");
        let _env = MasterEnv::set(None);

        // 没配主密钥：报错要能指路，不能只说「解密失败」。
        let err = decrypt_secret(&secret).expect_err("必须失败");
        assert!(err.to_string().contains(MASTER_KEY_ENV), "got: {err}");
    }

    #[test]
    fn wrong_master_key_cannot_decrypt() {
        let _guard = lock();
        let _env = MasterEnv::set(Some("key-a-aaaaaaaaaaaaaaaaaaaa"));
        let secret = encrypt_secret("sk-master").expect("encrypt");
        let _env = MasterEnv::set(Some("key-b-bbbbbbbbbbbbbbbbbbbb"));
        assert!(decrypt_secret(&secret).is_err(), "换主密钥必须解不开");
    }

    #[test]
    fn v1_secret_still_decrypts_after_switching_to_master_key() {
        let _guard = lock();
        let _env = MasterEnv::set(None);
        let legacy = encrypt_secret("sk-legacy").expect("encrypt");
        assert!(legacy.as_str().starts_with("enc:v1:"));

        // 切到主密钥模式：老数据（v1）仍要能解，新数据走 v2。
        let _env = MasterEnv::set(Some("unit-test-master-key-0123456789"));
        assert_eq!(decrypt_secret(&legacy).expect("legacy decrypt"), "sk-legacy");
        let fresh = encrypt_secret("sk-new").expect("encrypt");
        assert!(fresh.as_str().starts_with("enc:v2:"));
    }

    #[test]
    fn same_plaintext_encrypts_differently() {
        let _guard = lock();
        let _env = MasterEnv::set(None);
        let a = encrypt_secret("sk-same").expect("a");
        let b = encrypt_secret("sk-same").expect("b");
        assert_ne!(a, b, "random nonce must produce different ciphertext");
        assert_eq!(decrypt_secret(&a).expect("a"), decrypt_secret(&b).expect("b"));
    }

    #[test]
    fn tampered_secret_fails_to_decrypt() {
        let _guard = lock();
        let _env = MasterEnv::set(None);
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
        let _guard = lock();
        let _env = MasterEnv::set(None);
        assert!(encrypt_secret("").is_err());
        assert!(encrypt_secret("   ").is_err());
    }
}
