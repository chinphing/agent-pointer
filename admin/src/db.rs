//! 控制面数据库：sqlx + `Any` 驱动，运行期按连接串选后端。
//!
//! 支持的 URL 前缀：`sqlite:` / `postgres:`（或 `postgresql:`）/ `mysql:`。
//! SQL 一律用 `?` 占位，PG 下由 [`AdminStore::sql`] 改写成 `$1..$n`。

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use sqlx::any::{AnyPoolOptions, AnyRow};
use sqlx::{Any, AnyPool};
use std::sync::Once;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

// ---------------------------------------------------------------------------
// 连接池参数
//
// 按 Druid 的语义对齐（maxActive / minIdle / maxWait / testOnBorrow /
// connectionInitSqls）。sqlx 只给了旋钮，没有 Druid 那套"帮你兜住"的后台机制，
// 所以另外补了两块：启动预热（等价 initialSize）和保活任务（等价 keepAlive）。
//
// 为什么 min 不是 0：sqlx 默认 `min_connections = 0` + `idle_timeout = 10min`，
// 意味着空闲 10 分钟后池子归零，下一次请求要重建连接。
// ---------------------------------------------------------------------------

/// 等价 Druid `maxActive`。管理后台用户个位数，一个页面并发 2~3 个请求，10 够用。
const POOL_MAX: u32 = 10;
/// 等价 Druid `minIdle`。常驻两条：一条给保活，一条给用户请求。
const POOL_MIN: u32 = 2;
/// 等价 Druid `maxWait`。默认 30 秒太久，用户等不了。
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);
/// 保活间隔（秒）。必须短于环境里 NAT / RDS Proxy 的空闲回收时间（NAT 约 350 秒）。
/// 用 `POINTER_ADMIN_KEEPALIVE_SECS` 覆盖，`0` 表示关闭。
pub const POOL_KEEPALIVE_EVERY_SECS: u64 = 240;

/// 等价 Druid `connectionInitSqls`：连接建立后立刻执行的会话设置。
///
/// `wait_timeout` 让 MySQL 别在 8 小时静默后踢人；权限不足时静默忽略。
const SESSION_INIT_SQL: &[&str] = &[
    "SET NAMES utf8mb4",
    "SET SESSION wait_timeout = 28800",
    "SET SESSION interactive_timeout = 28800",
];

fn install_drivers() {
    static ONCE: Once = Once::new();
    ONCE.call_once(sqlx::any::install_default_drivers);
}

/// 按后端构造池参数。SQLite 单文件只留一条连接（per-connection PRAGMA 的麻烦）。
fn pool_options(backend: Backend) -> AnyPoolOptions {
    let mut opts = AnyPoolOptions::new()
        .max_connections(if backend == Backend::Sqlite { 1 } else { POOL_MAX })
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        // 等价 Druid testOnBorrow：取连接前 ping 一次。内网成本 <1ms，
        // 换来的是"拿到的一定是活连接"。
        .test_before_acquire(true);

    if backend != Backend::Sqlite {
        opts = opts
            .min_connections(POOL_MIN)
            // 不因空闲关闭：靠 min_connections 常驻 + max_lifetime 轮换。
            // （Druid 的 minEvictableIdleTimeMillis 只淘汰超出 minIdle 的部分，
            //   sqlx 没有这个"只淘汰多余"的语义，直接关掉更可控。）
            .idle_timeout(None)
            // 不做生命周期轮换（对齐 Druid `phyTimeoutMillis = -1` 的默认值）。
            // 原来解决的两件事都已由别的机制接管：空闲被对端杀 → 保活任务；
            // 死连接 → test_before_acquire。留下它只是每 30 分钟白重连一次。
            .max_lifetime(None)
            // 取连接耗时默认不打日志——线上出问题时看不到慢在哪，这里打开。
            .acquire_time_level(log::LevelFilter::Debug)
            .after_connect(|conn, _| {
                Box::pin(async move {
                    for sql in SESSION_INIT_SQL {
                        if let Err(e) = sqlx::query(sql).execute(&mut *conn).await {
                            log::debug!("会话初始化语句被忽略（{sql}）：{e}");
                        }
                    }
                    Ok(())
                })
            });
    }
    opts
}

impl AdminStore {
    /// 连接并迁移。`url` 决定后端。
    pub async fn connect(url: &str) -> Result<Self> {
        install_drivers();
        let backend = Backend::from_url(url)?;

        if backend == Backend::Sqlite {
            ensure_sqlite_file(url)?;
        }

        let pool = pool_options(backend)
            .connect(url)
            .await
            .with_context(|| format!("connect admin db ({url})"))?;

        // 启动预热（等价 Druid initialSize）：sqlx 的 min_connections 是后台
        // 任务懒补齐的，connect 之后并不会主动建。不预热的话第一个用户请求
        // 要自己付建连成本。
        if backend != Backend::Sqlite {
            let mut held = Vec::with_capacity(POOL_MIN as usize);
            for _ in 0..POOL_MIN {
                match pool.acquire().await {
                    Ok(conn) => held.push(conn),
                    Err(e) => {
                        log::warn!("连接池预热失败（不影响启动）：{e}");
                        break;
                    }
                }
            }
            let warmed = held.len();
            drop(held);
            log::info!(
                "连接池就绪：max={POOL_MAX} min={POOL_MIN} 预热={warmed} 条 \
                 acquire_timeout={}s max_lifetime=不轮换",
                POOL_ACQUIRE_TIMEOUT.as_secs()
            );
        }

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

    /// 后台保活（等价 Druid `keepAlive`）。
    ///
    /// sqlx 没有这个能力：空闲时池子不会自己 ping，而 NAT 网关（约 350 秒）
    /// 或 RDS Proxy 会把静默的连接回收掉。`test_before_acquire` 能保证拿到的
    /// 是活连接，但**发现死连接的那次请求要自己付建连成本**——保活把这一下
    /// 也省掉。
    ///
    /// SQLite 是本地文件，没有"对端超时"这回事，直接跳过。
    /// `every` 为零表示关闭。
    pub fn spawn_keepalive(&self, every: Duration) {
        if self.backend == Backend::Sqlite || every.is_zero() {
            return;
        }
        let store = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            // 错过的节拍不补，避免网络抖动后连续 ping 一串。
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // 第一次 tick 立即返回，跳过——预热刚做完，没必要马上再 ping。
            tick.tick().await;
            loop {
                tick.tick().await;
                match store.pool.acquire().await {
                    Ok(mut conn) => match sqlx::query("SELECT 1").execute(&mut *conn).await {
                        Ok(_) => log::debug!(
                            "连接池保活：ok size={} idle={}",
                            store.pool.size(),
                            store.pool.num_idle()
                        ),
                        Err(e) => log::warn!("连接池保活失败：{e}"),
                    },
                    Err(e) => log::warn!("连接池保活取连接失败：{e}"),
                }
            }
        });
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
    /// 池参数按 Druid 语义钉死：min=2 / max=10 / 不因空闲关闭 / 30 分钟轮换 /
    /// 3 秒取连接超时 / 取连接前校验。改这些数字前先想清楚线上会怎样。
    #[test]
    fn pool_options_follow_druid_semantics() {
        let o = pool_options(Backend::MySql);
        assert_eq!(o.get_max_connections(), 10, "Druid maxActive");
        assert_eq!(o.get_min_connections(), 2, "Druid minIdle：池子不归零");
        assert_eq!(
            o.get_acquire_timeout(),
            Duration::from_secs(3),
            "Druid maxWait"
        );
        assert_eq!(o.get_idle_timeout(), None, "空闲不关闭，靠 max_lifetime 轮换");
        assert_eq!(
            o.get_max_lifetime(),
            None,
            "对齐 Druid phyTimeout 默认值 -1：不做生命周期轮换"
        );
        assert!(o.get_test_before_acquire(), "Druid testOnBorrow");
    }

    #[test]
    fn sqlite_pool_stays_single_connection() {
        let o = pool_options(Backend::Sqlite);
        assert_eq!(o.get_max_connections(), 1);
        assert_eq!(o.get_min_connections(), 0, "SQLite 不做常驻与保活");
    }

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
