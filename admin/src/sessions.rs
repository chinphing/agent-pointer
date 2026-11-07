//! 控制面会话。
//!
//! 落库而不是内存：server 的 `WebSessionStore` 重启即掉线，管理端不能这样。
//! 库里只存 token 的 sha256，DB 泄漏也换不到可用会话。

use anyhow::Result;
use sqlx::{Any, Row};

use crate::db::{now_ts, sha256_hex, AdminStore};

pub const KIND_ADMIN: &str = "admin";
pub const KIND_MEMBER: &str = "member";
/// 30 天，与 server `web_session` 的 Max-Age 对齐。
pub const DEFAULT_TTL_SECS: i64 = 30 * 24 * 3600;

const TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user_id: String,
    pub kind: String,
    pub expires_at: i64,
}

fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    crate::db::hex_encode(&bytes)
}

fn token_id(token: &str) -> String {
    sha256_hex(token.as_bytes())
}

/// 建会话，返回**明文 token**（只此一次；库里是它的哈希）。
pub async fn create(
    store: &AdminStore,
    user_id: &str,
    kind: &str,
    ttl_secs: i64,
) -> Result<String> {
    let token = random_token();
    let now = now_ts();
    let sql = store.sql(
        "INSERT INTO sessions (id, user_id, kind, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(token_id(&token))
        .bind(user_id)
        .bind(kind)
        .bind(now)
        .bind(now + ttl_secs.max(1))
        .execute(store.pool())
        .await?;
    Ok(token)
}

/// 查会话；过期则顺手删掉并返回 `None`。
pub async fn lookup(store: &AdminStore, token: &str) -> Result<Option<Session>> {
    let id = token_id(token);
    let sql = store.sql("SELECT user_id, kind, expires_at FROM sessions WHERE id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(&id)
        .fetch_optional(store.pool())
        .await?;

    let Some(row) = row else { return Ok(None) };
    let session = Session {
        user_id: row.try_get(0)?,
        kind: row.try_get(1)?,
        expires_at: row.try_get(2)?,
    };
    if session.expires_at > now_ts() {
        return Ok(Some(session));
    }
    let del = store.sql("DELETE FROM sessions WHERE id = ?");
    sqlx::query::<Any>(&del).bind(&id).execute(store.pool()).await?;
    Ok(None)
}

pub async fn revoke(store: &AdminStore, token: &str) -> Result<bool> {
    let sql = store.sql("DELETE FROM sessions WHERE id = ?");
    let changed = sqlx::query::<Any>(&sql)
        .bind(token_id(token))
        .execute(store.pool())
        .await?
        .rows_affected();
    Ok(changed > 0)
}

pub async fn revoke_all_for_user(store: &AdminStore, user_id: &str) -> Result<u64> {
    let sql = store.sql("DELETE FROM sessions WHERE user_id = ?");
    Ok(sqlx::query::<Any>(&sql)
        .bind(user_id)
        .execute(store.pool())
        .await?
        .rows_affected())
}

pub async fn purge_expired(store: &AdminStore) -> Result<u64> {
    let sql = store.sql("DELETE FROM sessions WHERE expires_at <= ?");
    Ok(sqlx::query::<Any>(&sql)
        .bind(now_ts())
        .execute(store.pool())
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;
    use crate::schema::DEFAULT_TENANT;
    use crate::users;

    async fn store_with_user() -> (AdminStore, String) {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = users::create(&store, DEFAULT_TENANT, "sess-user", "", "pw-123456")
            .await
            .expect("create user")
            .id;
        (store, uid)
    }

    #[tokio::test]
    async fn create_and_lookup() {
        let (store, uid) = store_with_user().await;
        let token = create(&store, &uid, KIND_ADMIN, DEFAULT_TTL_SECS)
            .await
            .expect("create");
        assert_eq!(token.len(), TOKEN_BYTES * 2);

        let s = lookup(&store, &token).await.expect("lookup").expect("present");
        assert_eq!(s.user_id, uid);
        assert_eq!(s.kind, KIND_ADMIN);
        assert!(s.expires_at > now_ts());
    }

    #[tokio::test]
    async fn raw_token_is_not_stored() {
        let (store, uid) = store_with_user().await;
        let token = create(&store, &uid, KIND_ADMIN, DEFAULT_TTL_SECS)
            .await
            .expect("create");
        let row = store
            .fetch_optional("SELECT id FROM sessions")
            .await
            .expect("query")
            .expect("row");
        let stored: String = row.try_get(0).expect("id");
        assert_ne!(stored, token);
        assert_eq!(stored, token_id(&token));
    }

    #[tokio::test]
    async fn unknown_token_is_none() {
        let (store, _) = store_with_user().await;
        assert!(lookup(&store, "deadbeef").await.expect("lookup").is_none());
    }

    #[tokio::test]
    async fn revoke_removes_session() {
        let (store, uid) = store_with_user().await;
        let token = create(&store, &uid, KIND_MEMBER, DEFAULT_TTL_SECS)
            .await
            .expect("create");
        assert!(revoke(&store, &token).await.expect("revoke"));
        assert!(lookup(&store, &token).await.expect("lookup").is_none());
        assert!(!revoke(&store, &token).await.expect("revoke again"));
    }

    #[tokio::test]
    async fn expired_session_is_rejected_and_deleted() {
        let (store, uid) = store_with_user().await;
        let token = create(&store, &uid, KIND_ADMIN, DEFAULT_TTL_SECS)
            .await
            .expect("create");
        let sql = store.sql("UPDATE sessions SET expires_at = ?");
        sqlx::query::<Any>(&sql)
            .bind(now_ts() - 10)
            .execute(store.pool())
            .await
            .expect("expire");

        assert!(lookup(&store, &token).await.expect("lookup").is_none());
        let row = store
            .fetch_optional("SELECT COUNT(*) FROM sessions")
            .await
            .expect("count")
            .expect("row");
        let count: i64 = row.try_get(0).expect("count");
        assert_eq!(count, 0, "expired row should be deleted on lookup");
    }

    #[tokio::test]
    async fn purge_expired_only_removes_stale_rows() {
        let (store, uid) = store_with_user().await;
        let live = create(&store, &uid, KIND_ADMIN, DEFAULT_TTL_SECS)
            .await
            .expect("live");
        let stale = create(&store, &uid, KIND_ADMIN, DEFAULT_TTL_SECS)
            .await
            .expect("stale");
        let sql = store.sql("UPDATE sessions SET expires_at = ? WHERE id = ?");
        sqlx::query::<Any>(&sql)
            .bind(now_ts() - 1)
            .bind(token_id(&stale))
            .execute(store.pool())
            .await
            .expect("expire");

        assert_eq!(purge_expired(&store).await.expect("purge"), 1);
        assert!(lookup(&store, &live).await.expect("lookup").is_some());
    }

    #[tokio::test]
    async fn revoke_all_for_user_clears_every_session() {
        let (store, uid) = store_with_user().await;
        create(&store, &uid, KIND_ADMIN, DEFAULT_TTL_SECS)
            .await
            .expect("a");
        create(&store, &uid, KIND_MEMBER, DEFAULT_TTL_SECS)
            .await
            .expect("b");
        assert_eq!(
            revoke_all_for_user(&store, &uid).await.expect("revoke"),
            2
        );
    }
}
