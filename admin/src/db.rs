//! 控制面数据库：sqlx + `Any` 驱动，运行期按连接串选后端。
//!
//! 支持的 URL 前缀：`sqlite:` / `postgres:`（或 `postgresql:`）/ `mysql:`。
//! SQL 一律用 `?` 占位，PG 下由 [`AdminStore::sql`] 改写成 `$1..$n`。

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::{Any, AnyPool};
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
    MySql,
}

impl Backend {
    pub fn from_url(url: &str) -> Result<Self> {
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("sqlite:") {
            Ok(Self::Sqlite)
        } else if lower.starts_with("postgres:") || lower.starts_with("postgresql:") {
            Ok(Self::Postgres)
        } else if lower.starts_with("mysql:") {
            Ok(Self::MySql)
        } else {
            Err(anyhow!(
                "unsupported admin db url (expect sqlite:/postgres:/mysql:): {url}"
            ))
        }
    }

    pub fn uses_dollar_placeholders(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// `SUM()` over an integer column comes back as **DECIMAL** on MySQL, and
    /// `sqlx::Any` cannot decode it:
    ///
    /// ```text
    /// Any driver does not support MySql type MySqlTypeInfo { type: NewDecimal, … }
    /// ```
    ///
    /// So every aggregate that is read back as `i64` has to be cast to an
    /// integer first — with a dialect-specific type name.
    pub fn int_cast(self, expr: &str) -> String {
        let ty = match self {
            Self::Sqlite => "INTEGER",
            Self::Postgres => "BIGINT",
            Self::MySql => "SIGNED",
        };
        format!("CAST({expr} AS {ty})")
    }
}

/// sqlx 不会自动创建 SQLite 文件；控制面首次启动应该自己把库建出来，
/// 否则用户会看到 "unable to open database file" 这种毫无指向性的报错。
fn ensure_sqlite_file(url: &str) -> Result<()> {
    let raw = url.trim().strip_prefix("sqlite:").unwrap_or(url);
    let raw = raw.split('?').next().unwrap_or(raw);
    let raw = raw.strip_prefix("//").unwrap_or(raw);
    let path = raw.trim();
    if path.is_empty() || path == ":memory:" {
        return Ok(());
    }
    let file = std::path::Path::new(path);
    if file.exists() {
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create db dir {}", parent.display()))?;
        }
    }
    std::fs::File::create(file).with_context(|| format!("create db file {path}"))?;
    Ok(())
}

/// 控制面数据库句柄。
#[derive(Clone)]
pub struct AdminStore {
    pool: AnyPool,
    pub backend: Backend,
}

fn install_drivers() {
    static ONCE: Once = Once::new();
    ONCE.call_once(sqlx::any::install_default_drivers);
}

impl AdminStore {
    /// 连接并迁移。`url` 决定后端。
    pub async fn connect(url: &str) -> Result<Self> {
        install_drivers();
        let backend = Backend::from_url(url)?;

        if backend == Backend::Sqlite {
            ensure_sqlite_file(url)?;
        }

        // SQLite 是单文件：单连接即可，省掉 per-connection PRAGMA 的麻烦。
        let max_connections = if backend == Backend::Sqlite { 1 } else { 5 };
        let pool = AnyPoolOptions::new()
            .max_connections(max_connections)
            .connect(url)
            .await
            .with_context(|| format!("connect admin db ({url})"))?;

        let store = Self { pool, backend };
        if backend == Backend::Sqlite {
            for stmt in [
                "PRAGMA journal_mode=WAL",
                "PRAGMA foreign_keys=ON",
                "PRAGMA busy_timeout=5000",
            ] {
                let _ = store.exec(stmt).await;
            }
        }
        schema::migrate(&store).await?;
        Ok(store)
    }

    /// 测试用：独立的内存库。
    pub async fn connect_in_memory() -> Result<Self> {
        Self::connect("sqlite::memory:").await
    }

    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// 把 `?` 占位符改写成目标后端语法。SQL 里不要出现带 `?` 的字符串字面量。
    pub fn sql(&self, raw: &str) -> String {
        rewrite_placeholders(raw, self.backend)
    }

    /// 把聚合表达式转成本后端能解码的整数（见 [`Backend::int_cast`]）。
    pub fn int_sum(&self, expr: &str) -> String {
        self.backend.int_cast(expr)
    }

    /// 执行一条无参语句，返回受影响行数。
    pub async fn exec(&self, raw: &str) -> Result<u64> {
        let sql = self.sql(raw);
        let result = sqlx::query::<Any>(&sql)
            .execute(&self.pool)
            .await
            .with_context(|| format!("exec: {raw}"))?;
        Ok(result.rows_affected())
    }

    /// 执行并取全部行。
    pub async fn fetch_all(&self, raw: &str) -> Result<Vec<AnyRow>> {
        let sql = self.sql(raw);
        Ok(sqlx::query::<Any>(&sql).fetch_all(&self.pool).await?)
    }

    /// 执行并取首行。
    pub async fn fetch_optional(&self, raw: &str) -> Result<Option<AnyRow>> {
        let sql = self.sql(raw);
        Ok(sqlx::query::<Any>(&sql)
            .fetch_optional(&self.pool)
            .await?)
    }
}

/// `?` → `$1`, `$2`, …；非 PG 原样返回。
pub fn rewrite_placeholders(raw: &str, backend: Backend) -> String {
    if !backend.uses_dollar_placeholders() {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len() + 8);
    let mut index = 0usize;
    for ch in raw.chars() {
        if ch == '?' {
            index += 1;
            out.push('$');
            out.push_str(&index.to_string());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub fn random_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex_encode(Sha256::digest(data).as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// MySQL 的 `SUM(int_col)` 返回 DECIMAL，`sqlx::Any` 解不了——所以读回 i64 的
    /// 聚合必须按后端转成整数。这条测试钉死三种方言的类型名。
    #[test]
    fn int_cast_uses_backend_specific_type() {
        assert_eq!(
            Backend::Sqlite.int_cast("SUM(x)"),
            "CAST(SUM(x) AS INTEGER)"
        );
        assert_eq!(
            Backend::Postgres.int_cast("SUM(x)"),
            "CAST(SUM(x) AS BIGINT)"
        );
        assert_eq!(Backend::MySql.int_cast("SUM(x)"), "CAST(SUM(x) AS SIGNED)");
    }

    #[test]
    fn backend_is_detected_from_url() {
        assert_eq!(Backend::from_url("sqlite::memory:").unwrap(), Backend::Sqlite);
        assert_eq!(
            Backend::from_url("sqlite:///var/lib/pointer/admin.db").unwrap(),
            Backend::Sqlite
        );
        assert_eq!(
            Backend::from_url("postgres://u:p@localhost/pointer").unwrap(),
            Backend::Postgres
        );
        assert_eq!(
            Backend::from_url("postgresql://u:p@localhost/pointer").unwrap(),
            Backend::Postgres
        );
        assert_eq!(
            Backend::from_url("mysql://u:p@localhost/pointer").unwrap(),
            Backend::MySql
        );
        assert!(Backend::from_url("mssql://x").is_err());
        assert!(Backend::from_url("").is_err());
    }

    #[test]
    fn placeholders_are_rewritten_for_postgres_only() {
        let raw = "INSERT INTO t (a, b) VALUES (?, ?)";
        assert_eq!(
            rewrite_placeholders(raw, Backend::Postgres),
            "INSERT INTO t (a, b) VALUES ($1, $2)"
        );
        assert_eq!(rewrite_placeholders(raw, Backend::Sqlite), raw);
        assert_eq!(rewrite_placeholders(raw, Backend::MySql), raw);
    }

    #[test]
    fn hex_and_sha256_are_lowercase_hex() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x1a]), "00ff1a");
        let digest = sha256_hex(b"abc");
        assert_eq!(digest.len(), 64);
        assert!(digest
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn random_id_is_32_hex_chars_and_unique() {
        let a = random_id();
        let b = random_id();
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn store_connects_and_reports_schema_version() {
        let store = AdminStore::connect_in_memory().await.expect("connect");
        assert_eq!(store.backend(), Backend::Sqlite);
        let row = store
            .fetch_optional("SELECT meta_value FROM meta WHERE meta_key = 'schema_version'")
            .await
            .expect("query")
            .expect("row");
        let version: String = sqlx::Row::try_get(&row, 0).expect("value");
        assert_eq!(version, schema::SCHEMA_VERSION.to_string());
    }
}
