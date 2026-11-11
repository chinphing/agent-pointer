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

pub const SCHEMA_VERSION: i64 = 16;
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
        ("idx_wallet_ledger_user", "wallet_ledger", "user_id"),
        ("idx_grants_resource", "resource_grants", "resource_id"),
        ("idx_grants_principal", "resource_grants", "principal_id"),
        ("idx_audit_created", "audit_log", "created_at"),
        ("idx_usage_user_day", "usage_daily", "user_id"),
        // 用量查询按 tenant + 日期区间过滤；没有这个索引就是全表扫。
        ("idx_usage_tenant_day", "usage_daily", "tenant_id, day"),
        // 会话列表按时间倒序翻页；缺了它就会全表扫 + 排序。
        (
            "idx_snapshot_tenant_time",
            "conversation_snapshots",
            "tenant_id, created_at, id",
        ),
        ("idx_usage_monthly", "usage_monthly", "tenant_id, month"),
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
const DDL_V6: &[&str] = &[
    // 登录验证码：答案存 sha256(answer + id)，单次有效，5 分钟过期。
    "CREATE TABLE IF NOT EXISTS captchas (
        id            VARCHAR(64) PRIMARY KEY,
        answer_hash   VARCHAR(64) NOT NULL,
        expires_at    BIGINT NOT NULL,
        used_at       BIGINT,
        created_at    BIGINT NOT NULL
    )",
];

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

/// v7：客户端自动更新。一个版本一行，产物按平台各一行。
/// `signature` 存 `.sig` 全文 —— tauri-plugin-updater 要求内联，不能给 URL。
/// 计费：钱包（每人一个余额）+ 流水 + 模型单价。
///
/// 金额一律用 **微元**（1 元 = 1_000_000）存整数，避免浮点误差；
/// 展示时再除以 1e6 转成元。
/// v9：计费口径对齐官网 —— 单一费率 + token 细分落库。
///
/// 官网按 `ceil3(raw × 基准价 × ratio ÷ 2²⁰)` 计费，输入输出不分价；上报的
/// token 有 prompt / completion / thinking / cached 细分，都要能查。
/// v10：费率口径 —— 官网存的是「倍数」，不是换算后的单价。
///
/// 每个模型一个 ratio（以基准模型 1.0 为参照），全局一个基准价（元 / MB）。
/// `ratio_micro` = ratio × 1e6（整数存，避免浮点误差）；老库按换算关系反推。
/// v11：用量月表 —— 年度/历史报表不再扫明细。
///
/// 建表时从 `usage_daily` 回填一次，保证月表与日表从同一时刻起口径一致；
/// 之后由 `usage::record` 双写维护。
const DDL_V11: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS usage_monthly (
        tenant_id  VARCHAR(64) NOT NULL DEFAULT 'default',
        month      VARCHAR(7) NOT NULL,
        user_id    VARCHAR(64) NOT NULL,
        model      VARCHAR(128) NOT NULL DEFAULT '',
        prompt_tokens     BIGINT NOT NULL DEFAULT 0,
        completion_tokens BIGINT NOT NULL DEFAULT 0,
        thinking_tokens   BIGINT NOT NULL DEFAULT 0,
        cached_tokens     BIGINT NOT NULL DEFAULT 0,
        CONSTRAINT uq_usage_monthly UNIQUE (tenant_id, user_id, model, month)
    )",
    "INSERT INTO usage_monthly
        (tenant_id, month, user_id, model, prompt_tokens, completion_tokens,
         thinking_tokens, cached_tokens)
     SELECT tenant_id, SUBSTR(day, 1, 7), user_id, model,
            SUM(prompt_tokens), SUM(completion_tokens),
            SUM(thinking_tokens), SUM(cached_tokens)
       FROM usage_daily
      GROUP BY tenant_id, SUBSTR(day, 1, 7), user_id, model",
];

/// v12：流水加幂等键。
///
/// 支付回调、用量上报都可能重放；没有唯一键时同一个业务事件会被记两次账。
/// `external_ref` 为 NULL 的旧流水不受影响（唯一索引允许多个 NULL）。
const DDL_V12: &[&str] = &[
    "ALTER TABLE wallet_ledger ADD COLUMN external_ref VARCHAR(128)",
    "CREATE UNIQUE INDEX uq_wallet_ledger_external ON wallet_ledger(tenant_id, external_ref)",
];

/// v13：手机号。
///
/// 开源仓只负责**存**与**维护**（成员页手工录入），登录方式仍只有账号密码；
/// 私有仓的短信登录复用这一列。唯一索引按租户，NULL 不占用（多行为 NULL 合法）。
const DDL_V13: &[&str] = &[
    "ALTER TABLE users ADD COLUMN phone VARCHAR(32)",
    "CREATE UNIQUE INDEX uq_users_tenant_phone ON users(tenant_id, phone)",
];

/// v14：产物内容哈希。
///
/// 同一个安装包经常要出现在多个版本里（只有 macOS 变了，Windows/Linux 的包原样
/// 沿用）。存了 sha256 就能在**上传时**发现内容相同、直接复用磁盘上的那份，
/// 也能让「沿用上一版」只复制一条记录而不动文件。
const DDL_V14: &[&str] = &[
    "ALTER TABLE app_release_artifacts ADD COLUMN sha256 VARCHAR(64) NOT NULL DEFAULT ''",
    "CREATE INDEX idx_artifacts_sha ON app_release_artifacts(sha256)",
];

/// v15：会话快照索引。
///
/// 客户端上报用量时会顺带传一份对话快照（zip）。正文落盘（`blob_path`），
/// 库里只存索引和小预览——跟 `resource_versions` 一个路子，避免给 MySQL
/// 换 MEDIUMTEXT，也让列表页只扫小字段。
const DDL_V15: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS conversation_snapshots (
        id                VARCHAR(64) PRIMARY KEY,
        tenant_id         VARCHAR(64) NOT NULL,
        user_id           VARCHAR(64) NOT NULL,
        conversation_id   VARCHAR(64) NOT NULL,
        agent_instance_id VARCHAR(64) NOT NULL,
        agent_role_id     VARCHAR(64),
        request_id        VARCHAR(128),
        model             VARCHAR(128) NOT NULL DEFAULT '',
        prompt_tokens     BIGINT NOT NULL DEFAULT 0,
        completion_tokens BIGINT NOT NULL DEFAULT 0,
        message_count     BIGINT NOT NULL DEFAULT 0,
        preview           TEXT$512 NOT NULL DEFAULT '',
        blob_path         VARCHAR(512),
        size_bytes        BIGINT NOT NULL DEFAULT 0,
        created_at        BIGINT NOT NULL
    )",
    // 同一条上报重放只存一份；request_id 为空的老客户端不受约束（NULL 不占唯一）。
    "CREATE UNIQUE INDEX uq_snapshot_request ON conversation_snapshots(tenant_id, request_id)",
    "CREATE INDEX idx_snapshot_conv ON conversation_snapshots(tenant_id, conversation_id)",
    "CREATE INDEX idx_snapshot_user ON conversation_snapshots(tenant_id, user_id, created_at)",
    // 列表页是「按租户过滤 + 按时间倒序 + LIMIT」。少了这个索引，MySQL 会把
    // 全部行拉出来再排序：500 万行时实测 3.5 秒；有索引是 0.04 毫秒。
    "CREATE INDEX idx_snapshot_tenant_time ON conversation_snapshots(tenant_id, created_at, id)",
];

/// v16：会话内容上报改成「会话 + 轮次」两张表。
///
/// 旧表每轮存一份**全量**快照：一个 N 轮的会话要存 O(N²) 条消息，第 N 轮的
/// 快照里前 N-1 轮又出现一遍。改成按轮存之后，每条消息只落一次。
///
/// 判定主会话用 `lead_agent_instance_id` 而不是布尔标记：lead 是会话的属性，
/// 每次上报都带同一个值，所以先到后到、重试、丢包都不会影响判定。
///
/// 产品决定不做历史兼容，旧表直接丢弃。
const DDL_V16: &[&str] = &[
    "DROP TABLE IF EXISTS conversation_snapshots",
    "CREATE TABLE IF NOT EXISTS conversation_threads (
        id                     VARCHAR(64) PRIMARY KEY,
        tenant_id              VARCHAR(64) NOT NULL,
        user_id                VARCHAR(64) NOT NULL,
        conversation_id        VARCHAR(64) NOT NULL,
        agent_instance_id      VARCHAR(64) NOT NULL,
        agent_role_id          VARCHAR(64),
        agent_name             VARCHAR(128),
        lead_agent_instance_id VARCHAR(64) NOT NULL DEFAULT '',
        title                  TEXT$512 NOT NULL DEFAULT '',
        model                  VARCHAR(128) NOT NULL DEFAULT '',
        turn_count             BIGINT NOT NULL DEFAULT 0,
        message_count          BIGINT NOT NULL DEFAULT 0,
        first_at               BIGINT NOT NULL,
        last_at                BIGINT NOT NULL,
        UNIQUE (tenant_id, conversation_id, agent_instance_id)
    )",
    "CREATE TABLE IF NOT EXISTS conversation_turns (
        id            VARCHAR(64) PRIMARY KEY,
        thread_id     VARCHAR(64) NOT NULL,
        tenant_id     VARCHAR(64) NOT NULL,
        run_id        VARCHAR(128) NOT NULL,
        seq           BIGINT NOT NULL,
        turn_at       BIGINT NOT NULL,
        preview       TEXT$512 NOT NULL DEFAULT '',
        message_count BIGINT NOT NULL DEFAULT 0,
        blob_path     VARCHAR(512),
        size_bytes    BIGINT NOT NULL DEFAULT 0,
        created_at    BIGINT NOT NULL,
        UNIQUE (thread_id, run_id)
    )",
    // 列表页「按租户过滤 + 最近活跃倒序」。
    "CREATE INDEX idx_threads_last ON conversation_threads(tenant_id, last_at)",
    // 抽屉里按 seq 倒序分页取轮次。
    "CREATE INDEX idx_turns_thread ON conversation_turns(thread_id, seq)",
];

const DDL_V10: &[&str] = &[
    "ALTER TABLE model_prices ADD COLUMN ratio_micro BIGINT NOT NULL DEFAULT 0",
    // 元/百万 = ratio × 1e6 ÷ 2²⁰ → ratio_micro = micro_per_million × 2²⁰ ÷ 1e6
    "UPDATE model_prices SET ratio_micro = (micro_per_million * 1048576 + 500000) / 1000000 WHERE ratio_micro = 0",
];

const DDL_V9: &[&str] = &[
    "ALTER TABLE model_prices ADD COLUMN micro_per_million BIGINT NOT NULL DEFAULT 0",
    // 老库里的两档价此时是同一个数（初始化照抄官网单费率），取输入档回填。
    "UPDATE model_prices SET micro_per_million = prompt_micro_per_million WHERE micro_per_million = 0",
    "ALTER TABLE usage_daily ADD COLUMN thinking_tokens BIGINT NOT NULL DEFAULT 0",
    "ALTER TABLE usage_daily ADD COLUMN cached_tokens BIGINT NOT NULL DEFAULT 0",
];

const DDL_V8: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS wallets (
        user_id     VARCHAR(64) PRIMARY KEY,
        tenant_id   VARCHAR(64) NOT NULL DEFAULT 'default',
        balance_micro BIGINT NOT NULL DEFAULT 0,
        updated_at  BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS wallet_ledger (
        id           VARCHAR(64) PRIMARY KEY,
        tenant_id    VARCHAR(64) NOT NULL DEFAULT 'default',
        user_id      VARCHAR(64) NOT NULL,
        kind         VARCHAR(16) NOT NULL,
        amount_micro BIGINT NOT NULL,
        balance_after_micro BIGINT NOT NULL,
        model        VARCHAR(128) NOT NULL DEFAULT '',
        prompt_tokens     BIGINT NOT NULL DEFAULT 0,
        completion_tokens BIGINT NOT NULL DEFAULT 0,
        note         TEXT$255 NOT NULL DEFAULT '',
        created_at   BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS model_prices (
        tenant_id  VARCHAR(64) NOT NULL DEFAULT 'default',
        model      VARCHAR(128) NOT NULL,
        prompt_micro_per_million     BIGINT NOT NULL DEFAULT 0,
        completion_micro_per_million BIGINT NOT NULL DEFAULT 0,
        updated_at BIGINT NOT NULL,
        CONSTRAINT uq_model_prices UNIQUE (tenant_id, model)
    )",
];

const DDL_V7: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS app_releases (
        id           VARCHAR(64) PRIMARY KEY,
        tenant_id    VARCHAR(64) NOT NULL,
        version      VARCHAR(64) NOT NULL,
        notes        TEXT NOT NULL,
        published    INTEGER NOT NULL DEFAULT 0,
        published_at BIGINT,
        created_at   BIGINT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS app_release_artifacts (
        id         VARCHAR(64) PRIMARY KEY,
        release_id VARCHAR(64) NOT NULL,
        kind       VARCHAR(32) NOT NULL,
        filename   VARCHAR(255) NOT NULL,
        stored_as  VARCHAR(255) NOT NULL,
        size       BIGINT NOT NULL,
        signature  TEXT NOT NULL,
        created_at BIGINT NOT NULL
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
    if read_version(store).await? < 6 {
        for stmt in DDL_V6 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v6: {}", first_line(stmt)))?;
        }
        write_version(store, 6).await?;
    }
    if read_version(store).await? < 7 {
        for stmt in DDL_V7 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v7: {}", first_line(stmt)))?;
        }
        write_version(store, 7).await?;
    }
    if read_version(store).await? < 8 {
        for stmt in DDL_V8 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v8: {}", first_line(stmt)))?;
        }
        write_version(store, 8).await?;
    }
    if read_version(store).await? < 9 {
        for stmt in DDL_V9 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v9: {}", first_line(stmt)))?;
        }
        write_version(store, 9).await?;
    }
    if read_version(store).await? < 10 {
        for stmt in DDL_V10 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v10: {}", first_line(stmt)))?;
        }
        write_version(store, 10).await?;
    }
    if read_version(store).await? < 11 {
        for stmt in DDL_V11 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v11: {}", first_line(stmt)))?;
        }
        write_version(store, 11).await?;
    }
    if read_version(store).await? < 12 {
        for stmt in DDL_V12 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v12: {}", first_line(stmt)))?;
        }
        write_version(store, 12).await?;
    }
    if read_version(store).await? < 13 {
        for stmt in DDL_V13 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v13: {}", first_line(stmt)))?;
        }
        write_version(store, 13).await?;
    }
    if read_version(store).await? < 14 {
        for stmt in DDL_V14 {
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v14: {}", first_line(stmt)))?;
        }
        write_version(store, 14).await?;
    }

    if read_version(store).await? < 15 {
        for stmt in DDL_V15 {
            if is_index_ddl(stmt) {
                // 索引是纯优化，失败不影响正确性（MySQL 重复建索引会报错）。
                let _ = store.exec(&adapt(stmt, store.backend)).await;
                continue;
            }
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v15: {}", first_line(stmt)))?;
        }
        write_version(store, 15).await?;
    }

    if read_version(store).await? < 16 {
        for stmt in DDL_V16 {
            if is_index_ddl(stmt) {
                // 索引是纯优化，失败不影响正确性（MySQL 重复建索引会报错）。
                let _ = store.exec(&adapt(stmt, store.backend)).await;
                continue;
            }
            store
                .exec(&adapt(stmt, store.backend))
                .await
                .with_context(|| format!("apply admin schema v16: {}", first_line(stmt)))?;
        }
        write_version(store, 16).await?;
    }

    // 私有仓的迁移段：版本号从 9000 起，与开源仓的 1..n 互不干扰。
    migrate_ext(store).await?;

    for stmt in index_statements(store.backend()) {
        // 重复创建索引在 MySQL 会报错；索引是纯优化，失败不影响正确性。
        let _ = store.exec(&stmt).await;
    }

    seed_default_tenant(store).await?;
    rbac::seed_builtin_roles(store, DEFAULT_TENANT).await?;
    Ok(())
}

/// 这条 DDL 是不是在建索引。
fn is_index_ddl(stmt: &str) -> bool {
    let head = stmt.trim_start().to_ascii_uppercase();
    head.starts_with("CREATE INDEX") || head.starts_with("CREATE UNIQUE INDEX")
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

/// 跑私有仓的迁移段。版本号记在 `meta.ext_schema_version`，与开源仓的
/// `meta.schema_version` 分开，两边可以各自推进不撞号。
async fn migrate_ext(store: &AdminStore) -> Result<()> {
    let current = read_meta_i64(store, "ext_schema_version").await?;
    let mut applied = current;
    for (version, ddl) in crate::ext::migrations() {
        if *version <= current {
            continue;
        }
        let stmt = adapt(ddl, store.backend);
        // 索引是纯优化：重复创建（老库、中途失败重跑）不该让整个迁移失败。
        // 开源仓自己建索引也是这个策略（`let _ = store.exec(...)`）。
        // 不这么做的话，迁移在中途失败一次就会永久卡住——版本号没写进去，
        // 下次重跑又会撞上「索引已存在」。
        if is_index_ddl(ddl) {
            if let Err(e) = store.exec(&stmt).await {
                log::warn!("ext 迁移 v{version} 建索引失败（已忽略）：{e}");
            }
        } else {
            store
                .exec(&stmt)
                .await
                .with_context(|| format!("apply ext schema v{version}: {}", first_line(ddl)))?;
        }
        applied = *version;
    }
    if applied != current {
        write_meta_i64(store, "ext_schema_version", applied).await?;
    }
    Ok(())
}

async fn read_meta_i64(store: &AdminStore, key: &str) -> Result<i64> {
    let sql = store.sql("SELECT meta_value FROM meta WHERE meta_key = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(key)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(0) };
    let raw: String = row.try_get(0)?;
    Ok(raw.trim().parse().unwrap_or(0))
}

async fn write_meta_i64(store: &AdminStore, key: &str, version: i64) -> Result<()> {
    let sql = store.sql("UPDATE meta SET meta_value = ? WHERE meta_key = ?");
    let updated = sqlx::query::<Any>(&sql)
        .bind(version.to_string())
        .bind(key)
        .execute(store.pool())
        .await?
        .rows_affected();
    if updated == 0 {
        let sql = store.sql("INSERT INTO meta (meta_key, meta_value) VALUES (?, ?)");
        sqlx::query::<Any>(&sql)
            .bind(key)
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
