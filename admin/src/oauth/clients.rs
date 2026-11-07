//! OAuth 客户端登记与校验。
//!
//! 只存 `sha256(secret)`；`redirect_uri` 白名单按行存储。
//! 桌面客户端是 loopback 应用（RFC 8252），端口每次不同，所以按 `127.0.0.1/localhost` 前缀放行。

use anyhow::Result;
use sqlx::{Any, Row};

use crate::db::{now_ts, sha256_hex, AdminStore};

pub const CLIENT_SERVER: &str = "pointer-server";
pub const CLIENT_DESKTOP: &str = "pointer-desktop";

pub const KIND_SERVER: &str = "server";
pub const KIND_DESKTOP: &str = "desktop";

pub async fn upsert(
    store: &AdminStore,
    client_id: &str,
    secret: &str,
    kind: &str,
    redirect_uris: &[&str],
) -> Result<()> {
    let hash = sha256_hex(secret.as_bytes());
    let uris = redirect_uris.join("\n");
    let sql = store.sql("SELECT client_id FROM oauth_clients WHERE client_id = ?");
    let found = sqlx::query::<Any>(&sql)
        .bind(client_id)
        .fetch_optional(store.pool())
        .await?;

    if found.is_some() {
        let sql = store.sql(
            "UPDATE oauth_clients SET client_secret_hash = ?, kind = ?, redirect_uris = ?
             WHERE client_id = ?",
        );
        sqlx::query::<Any>(&sql)
            .bind(&hash)
            .bind(kind)
            .bind(&uris)
            .bind(client_id)
            .execute(store.pool())
            .await?;
    } else {
        let sql = store.sql(
            "INSERT INTO oauth_clients (client_id, client_secret_hash, kind, redirect_uris, created_at)
             VALUES (?, ?, ?, ?, ?)",
        );
        sqlx::query::<Any>(&sql)
            .bind(client_id)
            .bind(&hash)
            .bind(kind)
            .bind(&uris)
            .bind(now_ts())
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

pub async fn exists(store: &AdminStore, client_id: &str) -> Result<bool> {
    let sql = store.sql("SELECT client_id FROM oauth_clients WHERE client_id = ?");
    Ok(sqlx::query::<Any>(&sql)
        .bind(client_id)
        .fetch_optional(store.pool())
        .await?
        .is_some())
}

/// 公共客户端（桌面端）：没有 client_secret，靠 PKCE 保证安全（RFC 8252）。
pub async fn is_public_client(store: &AdminStore, client_id: &str) -> Result<bool> {
    let sql = store.sql("SELECT kind FROM oauth_clients WHERE client_id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(client_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(false) };
    let kind: String = row.try_get(0)?;
    Ok(kind == KIND_DESKTOP)
}

pub async fn verify_secret(store: &AdminStore, client_id: &str, secret: &str) -> Result<bool> {
    let sql = store.sql("SELECT client_secret_hash FROM oauth_clients WHERE client_id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(client_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(false) };
    let stored: String = row.try_get(0)?;
    let given = sha256_hex(secret.as_bytes());
    Ok(constant_time_eq(stored.as_bytes(), given.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub async fn redirect_allowed(
    store: &AdminStore,
    client_id: &str,
    redirect_uri: &str,
) -> Result<bool> {
    let sql = store.sql("SELECT kind, redirect_uris FROM oauth_clients WHERE client_id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(client_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(false) };
    let kind: String = row.try_get(0)?;
    let uris: String = row.try_get(1)?;
    let uri = redirect_uri.trim();

    if uris.lines().any(|line| line.trim() == uri) {
        return Ok(true);
    }
    if kind == KIND_DESKTOP && is_loopback(uri) {
        return Ok(true);
    }
    Ok(false)
}

fn is_loopback(uri: &str) -> bool {
    uri.starts_with("http://127.0.0.1:")
        || uri.starts_with("http://localhost:")
        || uri.starts_with("http://[::1]:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    #[tokio::test]
    async fn secret_is_hashed_and_verified() {
        let store = store().await;
        upsert(&store, CLIENT_SERVER, "s3cret", KIND_SERVER, &["http://127.0.0.1:8787/auth/callback"])
            .await
            .expect("upsert");

        assert!(verify_secret(&store, CLIENT_SERVER, "s3cret").await.expect("ok"));
        assert!(!verify_secret(&store, CLIENT_SERVER, "wrong").await.expect("bad"));
        assert!(!verify_secret(&store, "unknown", "s3cret").await.expect("missing"));
    }

    #[tokio::test]
    async fn upsert_rotates_secret() {
        let store = store().await;
        upsert(&store, CLIENT_SERVER, "old", KIND_SERVER, &[]).await.expect("first");
        upsert(&store, CLIENT_SERVER, "new", KIND_SERVER, &[]).await.expect("second");
        assert!(!verify_secret(&store, CLIENT_SERVER, "old").await.expect("old"));
        assert!(verify_secret(&store, CLIENT_SERVER, "new").await.expect("new"));
    }

    #[tokio::test]
    async fn server_redirect_requires_exact_match() {
        let store = store().await;
        upsert(
            &store,
            CLIENT_SERVER,
            "s",
            KIND_SERVER,
            &["http://127.0.0.1:8787/auth/callback"],
        )
        .await
        .expect("upsert");

        assert!(redirect_allowed(&store, CLIENT_SERVER, "http://127.0.0.1:8787/auth/callback")
            .await
            .expect("exact"));
        assert!(!redirect_allowed(&store, CLIENT_SERVER, "http://evil.example/cb")
            .await
            .expect("other"));
        assert!(
            !redirect_allowed(&store, CLIENT_SERVER, "http://127.0.0.1:9999/cb")
                .await
                .expect("loopback but not listed"),
            "server 客户端不吃 loopback 通配"
        );
    }

    #[tokio::test]
    async fn desktop_allows_any_loopback_port() {
        let store = store().await;
        upsert(&store, CLIENT_DESKTOP, "s", KIND_DESKTOP, &[])
            .await
            .expect("upsert");

        for uri in [
            "http://127.0.0.1:51234/callback",
            "http://localhost:8080/cb",
            "http://[::1]:9000/cb",
        ] {
            assert!(
                redirect_allowed(&store, CLIENT_DESKTOP, uri).await.expect("ok"),
                "应放行 {uri}"
            );
        }
        assert!(!redirect_allowed(&store, CLIENT_DESKTOP, "https://evil.example/cb")
            .await
            .expect("bad"));
    }
}
