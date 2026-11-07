//! 表结构与迁移。
//!
//! 三条跨库约束（SQLite / PostgreSQL / MySQL 共用同一份 DDL）：
//! 1. 字符串主键用 `VARCHAR(n)`（MySQL 的 `TEXT` 不能做主键）；
//! 2. 时间戳用 `BIGINT` 存 unix 秒（不用各库不同的日期类型）；
//! 3. 避开保留字：用 `meta_key` / `meta_value` / `role_key`，不写 `key`。
//!
//! 多租户约定：业务表带 `tenant_id`，唯一约束含租户；单租户期恒为 `default`。

use anyhow::{Context, Result};
use sqlx::{Any, Row};

use crate::db::{now_ts, AdminStore, Backend};
use crate::rbac;

pub const SCHEMA_VERSION: i64 = 5;
pub const DEFAULT_TENANT: &str = "default";

const DDL_V1: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS meta (
        meta_key   VARCHAR(64) PRIMARY KEY,
        meta_value TEXT$255 NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS tenants (
        id         VARCHAR(64) PRIMARY KEY,
        name       VARCHAR(255) NOT NULL,
        status     VARCHAR(32) NOT NULL DEFAULT 'active',
        created_at BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS users (
        id            VARCHAR(64) PRIMARY KEY,
        tenant_id     VARCHAR(64) NOT NULL DEFAULT 'default',
        username      VARCHAR(128) NOT NULL,
        display_name  VARCHAR(255) NOT NULL DEFAULT '',
        password_hash TEXT$255 NOT NULL,
        status        VARCHAR(32) NOT NULL DEFAULT 'active',
        created_at    BIGINT NOT NULL,
        last_login_at BIGINT,
        CONSTRAINT uq_users_tenant_username UNIQUE (tenant_id, username)
    )",
    "CREATE TABLE IF NOT EXISTS roles (
        id         VARCHAR(64) PRIMARY KEY,
        tenant_id  VARCHAR(64) NOT NULL DEFAULT 'default',
        role_key   VARCHAR(64) NOT NULL,
        name       VARCHAR(255) NOT NULL,
        builtin    INTEGER NOT NULL DEFAULT 0,
        created_at BIGINT NOT NULL,
        CONSTRAINT uq_roles_tenant_key UNIQUE (tenant_id, role_key)
    )",
    "CREATE TABLE IF NOT EXISTS role_permissions (
        role_id    VARCHAR(64) NOT NULL,
        permission VARCHAR(64) NOT NULL,
        PRIMARY KEY (role_id, permission),
        CONSTRAINT fk_rp_role FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS user_roles (
        user_id VARCHAR(64) NOT NULL,
        role_id VARCHAR(64) NOT NULL,
        PRIMARY KEY (user_id, role_id),
        CONSTRAINT fk_ur_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
        CONSTRAINT fk_ur_role FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS sessions (
        id         VARCHAR(64) PRIMARY KEY,
        user_id    VARCHAR(64) NOT NULL,
        kind       VARCHAR(32) NOT NULL,
        created_at BIGINT NOT NULL,
        expires_at BIGINT NOT NULL,
        CONSTRAINT fk_sessions_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
    )",
];

/// v2：统一资源表 + 版本 + 授权。
///
/// 技能、模型、插件、模型服务、MCP、档位映射都走 `resources`，差异只在 `kind` 与 `payload_json`。
const DDL_V2: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS resources (
        id                 VARCHAR(64) PRIMARY KEY,
        tenant_id          VARCHAR(64) NOT NULL DEFAULT 'default',
        kind               VARCHAR(32) NOT NULL,
        slug               VARCHAR(128) NOT NULL,
        name               VARCHAR(255) NOT NULL,
        description        TEXT$1024 NOT NULL,
        parent_id          VARCHAR(64),
        enabled            INTEGER NOT NULL DEFAULT 0,
        payload_json       TEXT$8000 NOT NULL,
        secret_enc         TEXT$1024,
        current_version_id VARCHAR(64),
        created_at         BIGINT NOT NULL,
        updated_at         BIGINT NOT NULL,
        CONSTRAINT uq_resources_tenant_kind_slug UNIQUE (tenant_id, kind, slug)
    )",
    "CREATE TABLE IF NOT EXISTS resource_versions (
        id          VARCHAR(64) PRIMARY KEY,
        resource_id VARCHAR(64) NOT NULL,
        version     VARCHAR(32) NOT NULL,
        dir_hash    VARCHAR(64) NOT NULL,
        blob_path   VARCHAR(512),
        size_bytes  BIGINT,
        uploaded_by VARCHAR(64),
        created_at  BIGINT NOT NULL,
        CONSTRAINT uq_resource_versions UNIQUE (resource_id, version),
        CONSTRAINT fk_rv_resource FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE
    )",
    "CREATE TABLE IF NOT EXISTS resource_grants (
        id             VARCHAR(64) PRIMARY KEY,
        resource_id    VARCHAR(64) NOT NULL,
        principal_type VARCHAR(16) NOT NULL,
        principal_id   VARCHAR(64) NOT NULL DEFAULT '',
        CONSTRAINT uq_resource_grants UNIQUE (resource_id, principal_type, principal_id),
        CONSTRAINT fk_rg_resource FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE
    )",
];

/// v3：审计、用量、配额、OAuth 客户端。
const DDL_V3: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS audit_log (
        id            VARCHAR(64) PRIMARY KEY,
        tenant_id     VARCHAR(64) NOT NULL DEFAULT 'default',
        actor_user_id VARCHAR(64),
        actor_name    VARCHAR(128) NOT NULL DEFAULT '',
        action        VARCHAR(64) NOT NULL,
        target_type   VARCHAR(32) NOT NULL DEFAULT '',
        target_id     VARCHAR(64) NOT NULL DEFAULT '',
        before_json   TEXT$4000,
        after_json    TEXT$4000,
        created_at    BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS usage_daily (
        id                VARCHAR(64) PRIMARY KEY,
        tenant_id         VARCHAR(64) NOT NULL DEFAULT 'default',
        user_id           VARCHAR(64) NOT NULL,
        model             VARCHAR(128) NOT NULL DEFAULT '',
        day               VARCHAR(10) NOT NULL,
        prompt_tokens     BIGINT NOT NULL DEFAULT 0,
        completion_tokens BIGINT NOT NULL DEFAULT 0,
        CONSTRAINT uq_usage_daily UNIQUE (tenant_id, user_id, model, day)
    )",
    "CREATE TABLE IF NOT EXISTS quotas (
        user_id             VARCHAR(64) PRIMARY KEY,
        tenant_id           VARCHAR(64) NOT NULL DEFAULT 'default',
        monthly_token_limit BIGINT,
        over_action         VARCHAR(16) NOT NULL DEFAULT 'warn',
        updated_at          BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS oauth_clients (
        client_id         VARCHAR(64) PRIMARY KEY,
        client_secret_hash VARCHAR(128) NOT NULL,
        kind              VARCHAR(16) NOT NULL,
        redirect_uris     TEXT$1024 NOT NULL,
        created_at        BIGINT NOT NULL
    )",
];

fn index_statements(backend: Backend) -> Vec<String> {
    let defs = [
        ("idx_sessions_user", "sessions", "user_id"),
        ("idx_sessions_expires", "sessions", "expires_at"),
        ("idx_user_roles_user", "user_roles", "user_id"),
        ("idx_resources_kind", "resources", "kind"),
        ("idx_grants_resource", "resource_grants", "resource_id"),
        ("idx_grants_principal", "resource_grants", "principal_id"),
        ("idx_audit_created", "audit_log", "created_at"),
        ("idx_usage_user_day", "usage_daily", "user_id"),
    ];
    defs.iter()
        .map(|(name, table, column)| {
            // MySQL 不支持 CREATE INDEX IF NOT EXISTS，靠忽略「已存在」错误兜底。
            if backend == Backend::MySql {
                format!("CREATE INDEX {name} ON {table}({column})")
            } else {
                format!("CREATE INDEX IF NOT EXISTS {name} ON {table}({column})")
            }
        })
        .collect()
}

/// 把 DDL 里的文本列标记翻译成目标后端真正支持的写法。
///
/// `TEXT$255` 这类标记存在的原因：sqlx 的 `Any` 驱动把 MySQL 的 `TEXT`
/// （协议里就是 `BLOB`）映射成 `AnyTypeInfoKind::Blob`，而 `String` 只能从
/// `Text` 解码——于是 MySQL 上任何 `TEXT` 列都读不出来。MySQL 必须用
/// `VARCHAR(n)`；SQLite / PostgreSQL 保留 `TEXT`（无长度上限，不会因为
/// payload 变大而写失败）。
fn adapt(stmt: &str, backend: Backend) -> String {
    let mut out = String::with_capacity(stmt.len() + 16);
    let mut rest = stmt;
    while let Some(pos) = rest.find("TEXT$") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos + 5..];
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if backend == Backend::MySql {
            out.push_str(&format!("VARCHAR({digits})"));
        } else {
            out.push_str("TEXT");
        }
        rest = &tail[digits.len()..];
    }
    out.push_str(rest);
    out
}

/// v4：资源级「启用策略」。
///
/// 刻意用 `ALTER` 而不是写进 DDL_V1：老库和新库都走同一条路径，只有一份定义。
/// 值域：`optional`（默认，员工自己决定）/ `default_on`（下发后默认启用，员工可关）。
const DDL_V4: &[&str] = &[
    "ALTER TABLE resources ADD COLUMN activation VARCHAR(16) NOT NULL DEFAULT 'optional'",
];

/// v5：授权码落库（多实例下两个 OAuth 请求会落到不同进程）。
const DDL_V5: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS oauth_codes (
        code_hash     VARCHAR(64) PRIMARY KEY,
        client_id     VARCHAR(64) NOT NULL,
        user_id       VARCHAR(64) NOT NULL,
        redirect_uri  VARCHAR(1024) NOT NULL,
        code_challenge VARCHAR(128) NOT NULL,
        expires_at    BIGINT NOT NULL,
        used_at       BIGINT,
        created_at    BIGINT NOT NULL
    )",
];

pub async fn migrate(store: &AdminStore) -> Result<()> {
    store
        .exec(&adapt(
            "CREATE TABLE IF NOT EXISTS meta (
                meta_key   VARCHAR(64) PRIMARY KEY,
                meta_value TEXT$255 NOT NULL
            )",
            store.backend,
        ))
        .await
        .context("create meta table")?;

    let current = read_version(store).await?;
    if current < 1 {
        for stmt in DDL_V1 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v1: {}", first_line(stmt)))?;
        }
        write_version(store, 1).await?;
    }
    if read_version(store).await? < 2 {
        for stmt in DDL_V2 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v2: {}", first_line(stmt)))?;
        }
        write_version(store, 2).await?;
    }
    if read_version(store).await? < 3 {
        for stmt in DDL_V3 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v3: {}", first_line(stmt)))?;
        }
        write_version(store, 3).await?;
    }
    if read_version(store).await? < 4 {
        for stmt in DDL_V4 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v4: {}", first_line(stmt)))?;
        }
        write_version(store, 4).await?;
    }
    if read_version(store).await? < 5 {
        for stmt in DDL_V5 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v5: {}", first_line(stmt)))?;
        }
        write_version(store, 5).await?;
    }

    for stmt in index_statements(store.backend()) {
        // 重复创建索引在 MySQL 会报错；索引是纯优化，失败不影响正确性。
        let _ = store.exec(&stmt).await;
    }

    seed_default_tenant(store).await?;
    rbac::seed_builtin_roles(store, DEFAULT_TENANT).await?;
    Ok(())
}

fn first_line(stmt: &str) -> &str {
    stmt.lines().next().unwrap_or(stmt).trim()
}

async fn read_version(store: &AdminStore) -> Result<i64> {
    let row = store
        .fetch_optional("SELECT meta_value FROM meta WHERE meta_key = 'schema_version'")
        .await?;
    let Some(row) = row else { return Ok(0) };
    let raw: String = row.try_get(0)?;
    Ok(raw.trim().parse().unwrap_or(0))
}

async fn write_version(store: &AdminStore, version: i64) -> Result<()> {
    // 不用 ON CONFLICT（MySQL 语法不同），先更新再按需插入。
    let sql = store.sql("UPDATE meta SET meta_value = ? WHERE meta_key = 'schema_version'");
    let updated = sqlx::query::<Any>(&sql)
        .bind(version.to_string())
        .execute(store.pool())
        .await?
        .rows_affected();
    if updated == 0 {
        let sql = store.sql("INSERT INTO meta (meta_key, meta_value) VALUES (?, ?)");
        sqlx::query::<Any>(&sql)
            .bind("schema_version")
            .bind(version.to_string())
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

async fn seed_default_tenant(store: &AdminStore) -> Result<()> {
    let row = store
        .fetch_optional("SELECT id FROM tenants WHERE id = 'default'")
        .await?;
    if row.is_some() {
        return Ok(());
    }
    let sql = store.sql("INSERT INTO tenants (id, name, status, created_at) VALUES (?, ?, 'active', ?)");
    sqlx::query::<Any>(&sql)
        .bind(DEFAULT_TENANT)
        .bind("默认组织")
        .bind(now_ts())
        .execute(store.pool())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("connect")
    }

    #[test]
    fn adapt_maps_text_markers_per_backend() {
        let stmt = "CREATE TABLE t (a TEXT$255 NOT NULL, b TEXT$8000)";
        assert_eq!(
            adapt(stmt, Backend::MySql),
            "CREATE TABLE t (a VARCHAR(255) NOT NULL, b VARCHAR(8000))"
        );
        assert_eq!(
            adapt(stmt, Backend::Sqlite),
            "CREATE TABLE t (a TEXT NOT NULL, b TEXT)"
        );
        assert_eq!(
            adapt(stmt, Backend::Postgres),
            "CREATE TABLE t (a TEXT NOT NULL, b TEXT)"
        );
    }

    #[tokio::test]
    async fn migrate_sets_version_and_is_idempotent() {
        let store = store().await;
        migrate(&store).await.expect("second migrate");
        assert_eq!(read_version(&store).await.expect("version"), SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn default_tenant_exists_once() {
        let store = store().await;
        let row = store
            .fetch_optional("SELECT COUNT(*) FROM tenants")
            .await
            .expect("count")
            .expect("row");
        let count: i64 = row.try_get(0).expect("count");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn username_unique_per_tenant() {
        let store = store().await;
        store
            .exec(
                "INSERT INTO users (id, tenant_id, username, password_hash, created_at)
                 VALUES ('u1', 'default', 'alice', 'x', 0)",
            )
            .await
            .expect("first insert");
        let err = store
            .exec(
                "INSERT INTO users (id, tenant_id, username, password_hash, created_at)
                 VALUES ('u2', 'default', 'alice', 'x', 0)",
            )
            .await
            .expect_err("duplicate must fail");
        let msg = format!("{err:#}").to_lowercase();
        assert!(msg.contains("unique"), "got: {msg}");
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        let store = store().await;
        let err = store
            .exec("INSERT INTO user_roles (user_id, role_id) VALUES ('missing', 'missing')")
            .await
            .expect_err("orphan row must fail");
        let msg = format!("{err:#}").to_lowercase();
        assert!(msg.contains("foreign key"), "got: {msg}");
    }
}
