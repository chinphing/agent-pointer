//! argon2id 密码哈希。
//!
//! 现有 standalone 的账密是配置里单条无盐 HMAC（`pointer-core/src/local_auth.rs`），
//! 无法支撑多用户；控制面用 argon2id + 每用户随机盐。

use anyhow::{anyhow, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

/// 生成 PHC 格式哈希（`$argon2id$v=19$m=...`）。
pub fn hash_password(plain: &str) -> Result<String> {
    if plain.is_empty() {
        return Err(anyhow!("password must not be empty"));
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hash password: {e}"))
}

/// 校验；哈希损坏或格式不对一律返回 false，不区分原因。
pub fn verify_password(hash: &str, plain: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let hash = hash_password("s3cret-pass").expect("hash");
        assert!(verify_password(&hash, "s3cret-pass"));
        assert!(!verify_password(&hash, "s3cret-pas"));
        assert!(!verify_password(&hash, ""));
    }

    #[test]
    fn salts_are_random() {
        let a = hash_password("same").expect("a");
        let b = hash_password("same").expect("b");
        assert_ne!(a, b, "two hashes of the same password must differ");
        assert!(verify_password(&a, "same"));
        assert!(verify_password(&b, "same"));
    }

    #[test]
    fn empty_password_rejected() {
        assert!(hash_password("").is_err());
    }

    #[test]
    fn malformed_hash_is_not_a_match() {
        assert!(!verify_password("not-a-phc-hash", "whatever"));
    }
}
