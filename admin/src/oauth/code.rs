//! 授权码：落库（`oauth_codes`），5 分钟有效，一次性。
//!
//! 为什么落库而不是放进程内存：OAuth 登录是**两个独立请求**
//! （`/oauth/authorize` 发码 → `/auth/app/token` 换 token）。
//! 多实例部署时这两个请求会落到不同进程，内存里的码在另一台机器上不存在，
//! 表现为「登录时好时坏」——最难排查的那类故障。
//!
//! 库里只存 `sha256(code)`：即使库被读走，也换不出可用的授权码。
//! 一次性由一条原子 `UPDATE ... WHERE used_at IS NULL` 保证，
//! 并发换同一个码时只有一个请求能拿到 `rows_affected = 1`。

use anyhow::Result;
use base64::Engine;
use sha2::{Digest, Sha256};
use sqlx::Any;

use crate::db::{now_ts, random_id, AdminStore};

const CODE_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCode {
    pub client_id: String,
    pub user_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub expires_at: i64,
}

fn hash_code(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}

/// 发一个新授权码。返回的是明文码，库里只有哈希。
pub async fn issue(
    store: &AdminStore,
    client_id: &str,
    user_id: &str,
    redirect_uri: &str,
    code_challenge: &str,
) -> Result<String> {
    let code = random_id();
    let now = now_ts();

    // 顺手清掉过期码：没有后台清理任务，就在发码时做，成本可忽略。
    let cleanup = store.sql("DELETE FROM oauth_codes WHERE expires_at <= ?");
    let _ = sqlx::query::<Any>(&cleanup)
        .bind(now)
        .execute(store.pool())
        .await;

    let sql = store.sql(
        "INSERT INTO oauth_codes (code_hash, client_id, user_id, redirect_uri, code_challenge, \
         expires_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(hash_code(&code))
        .bind(client_id)
        .bind(user_id)
        .bind(redirect_uri)
        .bind(code_challenge)
        .bind(now + CODE_TTL_SECS)
        .bind(now)
        .execute(store.pool())
        .await?;

    Ok(code)
}

/// 消费授权码：不存在 / 已用过 / 已过期都返回 `None`。
///
/// 先做原子 UPDATE 抢占，只有抢到的那个请求能继续——并发重放最多成功一次。
pub async fn consume(store: &AdminStore, code: &str) -> Result<Option<PendingCode>> {
    let code = code.trim();
    if code.is_empty() {
        return Ok(None);
    }
    let code_hash = hash_code(code);
    let now = now_ts();

    let claim = store.sql(
        "UPDATE oauth_codes SET used_at = ? WHERE code_hash = ? AND used_at IS NULL AND expires_at > ?",
    );
    let claimed = sqlx::query::<Any>(&claim)
        .bind(now)
        .bind(&code_hash)
        .bind(now)
        .execute(store.pool())
        .await?
        .rows_affected();
    if claimed == 0 {
        return Ok(None);
    }

    let sql = store.sql(
        "SELECT client_id, user_id, redirect_uri, code_challenge, expires_at \
         FROM oauth_codes WHERE code_hash = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(&code_hash)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    use sqlx::Row;
    Ok(Some(PendingCode {
        client_id: row.try_get(0)?,
        user_id: row.try_get(1)?,
        redirect_uri: row.try_get(2)?,
        code_challenge: row.try_get(3)?,
        expires_at: row.try_get(4)?,
    }))
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
    use crate::schema;

    async fn store() -> std::sync::Arc<AdminStore> {
        let store = AdminStore::connect_in_memory().await.expect("store");
        schema::migrate(&store).await.expect("migrate");
        std::sync::Arc::new(store)
    }

    #[tokio::test]
    async fn code_is_single_use() {
        let store = store().await;
        let code = issue(&store, "server", "u1", "http://127.0.0.1/cb", "challenge")
            .await
            .expect("issue");

        let first = consume(&store, &code).await.expect("consume").expect("hit");
        assert_eq!(first.user_id, "u1");
        assert_eq!(first.client_id, "server");
        assert!(
            consume(&store, &code).await.expect("consume again").is_none(),
            "同一个码不能换两次"
        );
    }

    #[tokio::test]
    async fn concurrent_consume_yields_exactly_one_winner() {
        let store = store().await;
        let code = issue(&store, "server", "u1", "http://127.0.0.1/cb", "challenge")
            .await
            .expect("issue");

        let mut handles = Vec::new();
        for _ in 0..8 {
            let store = store.clone();
            let code = code.clone();
            handles.push(tokio::spawn(async move {
                consume(&store, &code).await.expect("consume").is_some()
            }));
        }
        let mut wins = 0;
        for handle in handles {
            if handle.await.expect("join") {
                wins += 1;
            }
        }
        assert_eq!(wins, 1, "并发重放只能有一个成功");
    }

    #[tokio::test]
    async fn unknown_code_is_none() {
        let store = store().await;
        assert!(consume(&store, "nope").await.expect("consume").is_none());
        assert!(consume(&store, "   ").await.expect("blank").is_none());
    }

    #[tokio::test]
    async fn expired_code_is_rejected() {
        let store = store().await;
        let code = issue(&store, "server", "u1", "http://127.0.0.1/cb", "challenge")
            .await
            .expect("issue");

        // 直接把过期时间改到过去，模拟 5 分钟已过。
        let sql = store.sql("UPDATE oauth_codes SET expires_at = ? WHERE code_hash = ?");
        sqlx::query::<Any>(&sql)
            .bind(now_ts() - 1)
            .bind(hash_code(&code))
            .execute(store.pool())
            .await
            .expect("expire");

        assert!(consume(&store, &code).await.expect("consume").is_none());
    }

    /// 多实例的核心断言：发码和换码走**两个独立的 store**（各自连接池、零共享内存），
    /// 只要它们连同一个库就能换成功。内存实现下这条必然失败。
    #[tokio::test]
    async fn code_issued_by_one_instance_is_consumed_by_another() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "pointer-oauth-codes-{}-{}.db",
            std::process::id(),
            now_ts()
        ));
        let url = format!("sqlite:{}", path.display());

        let a = AdminStore::connect(&url).await.expect("instance A");
        schema::migrate(&a).await.expect("migrate");
        let b = AdminStore::connect(&url).await.expect("instance B");

        let code = issue(&a, "server", "u1", "http://127.0.0.1/cb", "challenge")
            .await
            .expect("issue on A");

        let pending = consume(&b, &code)
            .await
            .expect("consume on B")
            .expect("B 应能换掉 A 发的码");
        assert_eq!(pending.user_id, "u1");

        // 一次性跨实例同样成立：A 不能再用同一个码。
        assert!(consume(&a, &code).await.expect("replay on A").is_none());

        let _ = std::fs::remove_file(&path);
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
