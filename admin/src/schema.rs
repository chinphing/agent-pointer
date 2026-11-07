//! 表结构与迁移。
//!
//! 多租户约定：所有业务表带 `tenant_id`，唯一索引含租户；单租户期恒为 `default`。

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::now_ts;
use crate::rbac;

pub const SCHEMA_VERSION: i64 = 1;
pub const DEFAULT_TENANT: &str = "default";

const DDL_V1: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tenants (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'active',
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id            TEXT PRIMARY KEY,
    tenant_id     TEXT NOT NULL DEFAULT 'default',
    username      TEXT NOT NULL,
    display_name  TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'active',
    created_at    INTEGER NOT NULL,
    last_login_at INTEGER,
    UNIQUE (tenant_id, username)
);

CREATE TABLE IF NOT EXISTS roles (
    id         TEXT PRIMARY KEY,
    tenant_id  TEXT NOT NULL DEFAULT 'default',
    key        TEXT NOT NULL,
    name       TEXT NOT NULL,
    builtin    INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    UNIQUE (tenant_id, key)
);

CREATE TABLE IF NOT EXISTS role_permissions (
    role_id    TEXT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (role_id, permission)
);

CREATE TABLE IF NOT EXISTS user_roles (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id TEXT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);

CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);
CREATE INDEX IF NOT EXISTS idx_user_roles_user ON user_roles(user_id);
"#;

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;

    let current = read_version(conn)?;
    if current < 1 {
        conn.execute_batch(DDL_V1).context("apply admin schema v1")?;
        write_version(conn, 1)?;
    }

    seed_default_tenant(conn)?;
    rbac::seed_builtin_roles(conn, DEFAULT_TENANT)?;
    Ok(())
}

fn read_version(conn: &Connection) -> Result<i64> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|s| s.trim().parse().ok()).unwrap_or(0))
}

fn write_version(conn: &Connection, version: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![version.to_string()],
    )?;
    Ok(())
}

fn seed_default_tenant(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO tenants (id, name, status, created_at)
         VALUES (?1, ?2, 'active', ?3)",
        params![DEFAULT_TENANT, "默认组织", now_ts()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_conn;

    #[test]
    fn migrate_sets_version_and_is_idempotent() {
        let conn = test_conn();
        // 第二次迁移不应报错，也不应重复插入。
        migrate(&conn).expect("second migrate");
        let version = read_version(&conn).expect("version");
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn default_tenant_exists_once() {
        let conn = test_conn();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tenants", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 1);
        let id: String = conn
            .query_row("SELECT id FROM tenants LIMIT 1", [], |r| r.get(0))
            .expect("tenant id");
        assert_eq!(id, DEFAULT_TENANT);
    }

    #[test]
    fn users_unique_per_tenant() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO users (id, tenant_id, username, password_hash, created_at)
             VALUES ('u1', 'default', 'alice', 'x', 0)",
            [],
        )
        .expect("first insert");
        let err = conn
            .execute(
                "INSERT INTO users (id, tenant_id, username, password_hash, created_at)
                 VALUES ('u2', 'default', 'alice', 'x', 0)",
                [],
            )
            .expect_err("duplicate username must fail");
        assert!(err.to_string().contains("UNIQUE"), "got: {err}");
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let conn = test_conn();
        let err = conn
            .execute(
                "INSERT INTO user_roles (user_id, role_id) VALUES ('missing', 'missing')",
                [],
            )
            .expect_err("orphan user_roles must fail");
        assert!(err.to_string().to_lowercase().contains("foreign key"), "got: {err}");
    }
}
