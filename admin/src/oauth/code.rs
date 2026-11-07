//! 授权码（内存，5 分钟有效，一次性）。
//!
//! 不落库：授权码寿命只有几分钟，单进程内存足够；
//! 多实例部署时才需要换成 Redis/DB（写在这里是显式取舍，不是遗漏）。

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::db::{now_ts, random_id};

const CODE_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCode {
    pub client_id: String,
    pub user_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub expires_at: i64,
}

#[derive(Default)]
pub struct CodeStore {
    inner: Mutex<HashMap<String, PendingCode>>,
}

impl CodeStore {
    pub fn issue(
        &self,
        client_id: &str,
        user_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
    ) -> String {
        let code = random_id();
        let pending = PendingCode {
            client_id: client_id.to_string(),
            user_id: user_id.to_string(),
            redirect_uri: redirect_uri.to_string(),
            code_challenge: code_challenge.to_string(),
            expires_at: now_ts() + CODE_TTL_SECS,
        };
        let mut guard = self.inner.lock().expect("code store poisoned");
        guard.retain(|_, v| v.expires_at > now_ts());
        guard.insert(code.clone(), pending);
        code
    }

    /// 取出即删除：同一个授权码不能换两次。
    pub fn consume(&self, code: &str) -> Option<PendingCode> {
        let mut guard = self.inner.lock().expect("code store poisoned");
        let pending = guard.remove(code)?;
        if pending.expires_at <= now_ts() {
            return None;
        }
        Some(pending)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// PKCE S256 校验。`challenge` 为空视为未启用（由调用方决定是否放行）。
pub fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    let challenge = challenge.trim();
    if challenge.is_empty() {
        return false;
    }
    if verifier.trim().is_empty() {
        return false;
    }
    let digest = Sha256::digest(verifier.as_bytes());
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    computed == challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_single_use() {
        let store = CodeStore::default();
        let code = store.issue("server", "u1", "http://127.0.0.1/cb", "challenge");
        assert_eq!(store.len(), 1);

        let first = store.consume(&code).expect("first consume");
        assert_eq!(first.user_id, "u1");
        assert!(store.consume(&code).is_none(), "同一个码不能换两次");
        assert!(store.is_empty());
    }

    #[test]
    fn unknown_code_is_none() {
        let store = CodeStore::default();
        assert!(store.consume("nope").is_none());
    }

    #[test]
    fn pkce_s256_matches_rfc7636_vector() {
        // RFC 7636 附录 B
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_pkce(verifier, challenge));
        assert!(!verify_pkce("wrong-verifier", challenge));
        assert!(!verify_pkce(verifier, ""), "空 challenge 一律不通过");
        assert!(!verify_pkce("", challenge));
    }
}
