//! 成员账号。
//!
//! 密码只以 argon2id 哈希入库；`User` 结构体刻意不含 `password_hash`，避免误传给接口层。

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::{now_ts, random_id};
use crate::password;

pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_DISABLED: &str = "disabled";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub tenant_id: String,
    pub username: String,
    pub display_name: String,
    pub status: String,
    pub created_at: i64,
    pub last_login_at: Option<i64>,
}

impl User {
    pub fn is_active(&self) -> bool {
        self.status == STATUS_ACTIVE
    }
}

const COLS: &str = "id, tenant_id, username, display_name, status, created_at, last_login_at";

fn map_user(r: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        tenant_id: r.get(1)?,
        username: r.get(2)?,
        display_name: r.get(3)?,
        status: r.get(4)?,
        created_at: r.get(5)?,
        last_login_at: r.get(6)?,
    })
}

fn normalize_username(raw: &str) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(anyhow!("username must not be empty"));
    }
    if name.chars().count() > 64 {
        return Err(anyhow!("username too long (max 64)"));
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(anyhow!("username must not contain whitespace"));
    }
    Ok(name.to_string())
}

pub fn create(
    conn: &Connection,
    tenant_id: &str,
    username: &str,
    display_name: &str,
    plain_password: &str,
) -> Result<User> {
    let username = normalize_username(username)?;
    let display_name = {
        let d = display_name.trim();
        if d.is_empty() {
            username.clone()
        } else {
            d.to_string()
        }
    };
    if find_by_username(conn, tenant_id, &username)?.is_some() {
        return Err(anyhow!("username already exists: {username}"));
    }
    let password_hash = password::hash_password(plain_password)?;
    let id = random_id();
    conn.execute(
        "INSERT INTO users (id, tenant_id, username, display_name, password_hash, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            tenant_id,
            username,
            display_name,
            password_hash,
            STATUS_ACTIVE,
            now_ts()
        ],
    )?;
    get(conn, &id)?.ok_or_else(|| anyhow!("user vanished right after insert"))
}

pub fn find_by_username(
    conn: &Connection,
    tenant_id: &str,
    username: &str,
) -> Result<Option<User>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM users WHERE tenant_id = ?1 AND username = ?2"),
            params![tenant_id, username.trim()],
            map_user,
        )
        .optional()?)
}

pub fn get(conn: &Connection, user_id: &str) -> Result<Option<User>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM users WHERE id = ?1"),
            params![user_id],
            map_user,
        )
        .optional()?)
}

pub fn list(conn: &Connection, tenant_id: &str) -> Result<Vec<User>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM users WHERE tenant_id = ?1 ORDER BY username"
    ))?;
    let rows = stmt.query_map(params![tenant_id], map_user)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn set_status(conn: &Connection, user_id: &str, status: &str) -> Result<()> {
    if status != STATUS_ACTIVE && status != STATUS_DISABLED {
        return Err(anyhow!("unknown user status: {status}"));
    }
    let changed = conn.execute(
        "UPDATE users SET status = ?2 WHERE id = ?1",
        params![user_id, status],
    )?;
    if changed == 0 {
        return Err(anyhow!("user not found: {user_id}"));
    }
    Ok(())
}

pub fn reset_password(conn: &Connection, user_id: &str, plain_password: &str) -> Result<()> {
    let password_hash = password::hash_password(plain_password)?;
    let changed = conn.execute(
        "UPDATE users SET password_hash = ?2 WHERE id = ?1",
        params![user_id, password_hash],
    )?;
    if changed == 0 {
        return Err(anyhow!("user not found: {user_id}"));
    }
    Ok(())
}

pub fn touch_login(conn: &Connection, user_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE users SET last_login_at = ?2 WHERE id = ?1",
        params![user_id, now_ts()],
    )?;
    Ok(())
}

/// 校验账密。用户不存在、被禁用、密码错误一律返回 `None`（不泄露是哪一种）。
pub fn authenticate(
    conn: &Connection,
    tenant_id: &str,
    username: &str,
    plain_password: &str,
) -> Result<Option<User>> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, password_hash, status FROM users WHERE tenant_id = ?1 AND username = ?2",
            params![tenant_id, username.trim()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;

    let Some((id, hash, status)) = row else {
        return Ok(None);
    };
    if status != STATUS_ACTIVE {
        log::warn!("admin auth: user {id} is {status}");
        return Ok(None);
    }
    if !password::verify_password(&hash, plain_password) {
        return Ok(None);
    }
    touch_login(conn, &id)?;
    get(conn, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;
    use crate::schema::DEFAULT_TENANT;

    #[test]
    fn create_and_fetch() {
        let conn = test_conn();
        let u = create(&conn, DEFAULT_TENANT, "  alice  ", "", "pw-123456").expect("create");
        assert_eq!(u.username, "alice");
        assert_eq!(u.display_name, "alice");
        assert_eq!(u.status, STATUS_ACTIVE);
        assert!(u.last_login_at.is_none());

        let found = find_by_username(&conn, DEFAULT_TENANT, "alice")
            .expect("find")
            .expect("present");
        assert_eq!(found.id, u.id);
        assert_eq!(get(&conn, &u.id).expect("get").expect("present"), u);
    }

    #[test]
    fn duplicate_username_rejected() {
        let conn = test_conn();
        create(&conn, DEFAULT_TENANT, "bob", "Bob", "pw-123456").expect("first");
        let err = create(&conn, DEFAULT_TENANT, "bob", "Bob2", "pw-123456").expect_err("dup");
        assert!(err.to_string().contains("already exists"), "got: {err}");
    }

    #[test]
    fn invalid_usernames_rejected() {
        let conn = test_conn();
        for bad in ["", "   ", "a b", "x".repeat(65).as_str()] {
            assert!(
                create(&conn, DEFAULT_TENANT, bad, "", "pw-123456").is_err(),
                "expected reject for {bad:?}"
            );
        }
    }

    #[test]
    fn password_is_not_stored_in_plaintext() {
        let conn = test_conn();
        let u = create(&conn, DEFAULT_TENANT, "carol", "", "pw-123456").expect("create");
        let hash: String = conn
            .query_row(
                "SELECT password_hash FROM users WHERE id = ?1",
                params![u.id],
                |r| r.get(0),
            )
            .expect("hash");
        assert!(hash.starts_with("$argon2id$"), "got: {hash}");
        assert!(!hash.contains("pw-123456"));
    }

    #[test]
    fn authenticate_ok_and_updates_last_login() {
        let conn = test_conn();
        let u = create(&conn, DEFAULT_TENANT, "dave", "", "pw-123456").expect("create");
        let authed = authenticate(&conn, DEFAULT_TENANT, "dave", "pw-123456")
            .expect("auth")
            .expect("ok");
        assert_eq!(authed.id, u.id);
        assert!(authed.last_login_at.is_some());
    }

    #[test]
    fn authenticate_rejects_wrong_password_and_disabled_user() {
        let conn = test_conn();
        let u = create(&conn, DEFAULT_TENANT, "erin", "", "pw-123456").expect("create");
        assert!(authenticate(&conn, DEFAULT_TENANT, "erin", "wrong").expect("auth").is_none());

        set_status(&conn, &u.id, STATUS_DISABLED).expect("disable");
        assert!(
            authenticate(&conn, DEFAULT_TENANT, "erin", "pw-123456")
                .expect("auth")
                .is_none(),
            "disabled user must not authenticate"
        );
    }

    #[test]
    fn authenticate_unknown_user_is_none() {
        let conn = test_conn();
        assert!(authenticate(&conn, DEFAULT_TENANT, "ghost", "pw-123456")
            .expect("auth")
            .is_none());
    }

    #[test]
    fn reset_password_takes_effect() {
        let conn = test_conn();
        let u = create(&conn, DEFAULT_TENANT, "frank", "", "old-password").expect("create");
        reset_password(&conn, &u.id, "new-password").expect("reset");
        assert!(authenticate(&conn, DEFAULT_TENANT, "frank", "old-password")
            .expect("auth")
            .is_none());
        assert!(authenticate(&conn, DEFAULT_TENANT, "frank", "new-password")
            .expect("auth")
            .is_some());
    }

    #[test]
    fn set_status_rejects_unknown_value_and_missing_user() {
        let conn = test_conn();
        let u = create(&conn, DEFAULT_TENANT, "gina", "", "pw-123456").expect("create");
        assert!(set_status(&conn, &u.id, "archived").is_err());
        assert!(set_status(&conn, "missing", STATUS_DISABLED).is_err());
    }

    #[test]
    fn list_is_tenant_scoped_and_sorted() {
        let conn = test_conn();
        create(&conn, DEFAULT_TENANT, "zoe", "", "pw-123456").expect("zoe");
        create(&conn, DEFAULT_TENANT, "amy", "", "pw-123456").expect("amy");
        create(&conn, "other", "anna", "", "pw-123456").expect("other tenant");
        let names: Vec<String> = list(&conn, DEFAULT_TENANT)
            .expect("list")
            .into_iter()
            .map(|u| u.username)
            .collect();
        assert_eq!(names, vec!["amy".to_string(), "zoe".to_string()]);
    }
}
