//! 控制面会话。
//!
//! 落表而不是内存：server 的 `WebSessionStore` 重启即掉线，管理端不能这样。
//! 库里只存 token 的 sha256，DB 泄漏也换不到可用会话。

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::{now_ts, sha256_hex};

pub const KIND_ADMIN: &str = "admin";
pub const KIND_MEMBER: &str = "member";
/// 30 天，与 `web_session` 的 Max-Age 对齐。
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
pub fn create(conn: &Connection, user_id: &str, kind: &str, ttl_secs: i64) -> Result<String> {
    let token = random_token();
    let now = now_ts();
    conn.execute(
        "INSERT INTO sessions (id, user_id, kind, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            token_id(&token),
            user_id,
            kind,
            now,
            now + ttl_secs.max(1)
        ],
    )?;
    Ok(token)
}

/// 查会话；过期则顺手删掉并返回 `None`。
pub fn lookup(conn: &Connection, token: &str) -> Result<Option<Session>> {
    let id = token_id(token);
    let found: Option<Session> = conn
        .query_row(
            "SELECT user_id, kind, expires_at FROM sessions WHERE id = ?1",
            params![id],
            |r| {
                Ok(Session {
                    user_id: r.get(0)?,
                    kind: r.get(1)?,
                    expires_at: r.get(2)?,
                })
            },
        )
        .optional()?;

    match found {
        Some(s) if s.expires_at > now_ts() => Ok(Some(s)),
        Some(_) => {
            conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
            Ok(None)
        }
        None => Ok(None),
    }
}

pub fn revoke(conn: &Connection, token: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM sessions WHERE id = ?1",
        params![token_id(token)],
    )?;
    Ok(changed > 0)
}

pub fn revoke_all_for_user(conn: &Connection, user_id: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])?)
}

pub fn purge_expired(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM sessions WHERE expires_at <= ?1",
        params![now_ts()],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;
    use crate::schema::DEFAULT_TENANT;
    use crate::users;

    fn seed_user(conn: &Connection) -> String {
        users::create(conn, DEFAULT_TENANT, "sess-user", "", "pw-123456")
            .expect("create user")
            .id
    }

    #[test]
    fn create_and_lookup() {
        let conn = test_conn();
        let uid = seed_user(&conn);
        let token = create(&conn, &uid, KIND_ADMIN, DEFAULT_TTL_SECS).expect("create");
        assert_eq!(token.len(), TOKEN_BYTES * 2);

        let s = lookup(&conn, &token).expect("lookup").expect("present");
        assert_eq!(s.user_id, uid);
        assert_eq!(s.kind, KIND_ADMIN);
        assert!(s.expires_at > now_ts());
    }

    #[test]
    fn raw_token_is_not_stored() {
        let conn = test_conn();
        let uid = seed_user(&conn);
        let token = create(&conn, &uid, KIND_ADMIN, DEFAULT_TTL_SECS).expect("create");
        let stored: String = conn
            .query_row("SELECT id FROM sessions LIMIT 1", [], |r| r.get(0))
            .expect("id");
        assert_ne!(stored, token);
        assert_eq!(stored, token_id(&token));
    }

    #[test]
    fn unknown_token_is_none() {
        let conn = test_conn();
        assert!(lookup(&conn, "deadbeef").expect("lookup").is_none());
    }

    #[test]
    fn revoke_removes_session() {
        let conn = test_conn();
        let uid = seed_user(&conn);
        let token = create(&conn, &uid, KIND_MEMBER, DEFAULT_TTL_SECS).expect("create");
        assert!(revoke(&conn, &token).expect("revoke"));
        assert!(lookup(&conn, &token).expect("lookup").is_none());
        assert!(!revoke(&conn, &token).expect("revoke again"));
    }

    #[test]
    fn expired_session_is_rejected_and_deleted() {
        let conn = test_conn();
        let uid = seed_user(&conn);
        let token = create(&conn, &uid, KIND_ADMIN, DEFAULT_TTL_SECS).expect("create");
        conn.execute(
            "UPDATE sessions SET expires_at = ?1",
            params![now_ts() - 10],
        )
        .expect("expire");
        assert!(lookup(&conn, &token).expect("lookup").is_none());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 0, "expired row should be deleted on lookup");
    }

    #[test]
    fn purge_expired_only_removes_stale_rows() {
        let conn = test_conn();
        let uid = seed_user(&conn);
        let live = create(&conn, &uid, KIND_ADMIN, DEFAULT_TTL_SECS).expect("live");
        let stale = create(&conn, &uid, KIND_ADMIN, DEFAULT_TTL_SECS).expect("stale");
        conn.execute(
            "UPDATE sessions SET expires_at = ?1 WHERE id = ?2",
            params![now_ts() - 1, token_id(&stale)],
        )
        .expect("expire");

        assert_eq!(purge_expired(&conn).expect("purge"), 1);
        assert!(lookup(&conn, &live).expect("lookup").is_some());
    }

    #[test]
    fn revoke_all_for_user_clears_every_session() {
        let conn = test_conn();
        let uid = seed_user(&conn);
        create(&conn, &uid, KIND_ADMIN, DEFAULT_TTL_SECS).expect("a");
        create(&conn, &uid, KIND_MEMBER, DEFAULT_TTL_SECS).expect("b");
        assert_eq!(revoke_all_for_user(&conn, &uid).expect("revoke"), 2);
    }
}
