//! 用量与配额。
//!
//! 用量按 (用户 × 模型 × 天) 累加；`usage_daily` 的唯一键就是这四个字段。
//! 没有用 `ON CONFLICT`（MySQL 语法不同），改成「先查后写」，非原子但对管理端量级足够。

use anyhow::{anyhow, Result};
use sqlx::{Any, Row};

use crate::db::{now_ts, random_id, AdminStore};

pub const OVER_ACTION_WARN: &str = "warn";
pub const OVER_ACTION_BLOCK: &str = "block";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRow {
    pub user_id: String,
    pub model: String,
    pub day: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub thinking_tokens: i64,
    pub cached_tokens: i64,
}

impl UsageRow {
    pub fn total(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// 聚合结果：key + 四个 token 细分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAgg {
    pub key: String,
    pub prompt: i64,
    pub completion: i64,
    pub thinking: i64,
    pub cached: i64,
}

impl UsageAgg {
    pub fn total(&self) -> i64 {
        self.prompt + self.completion
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    User,
    Model,
    Day,
}

/// 累加一次上报。token 细分（prompt / completion / thinking / cached）全部落库。
///
/// 单条 upsert：一次往返、并发安全。旧实现是「先 SELECT 再 UPDATE/INSERT」，
/// 两个请求同时查不到就会都走 INSERT，撞唯一键或丢一条。
/// 同时累加进 `usage_monthly`，年度报表不用再扫明细。
#[allow(clippy::too_many_arguments)] // 一次上报的字段本来就多，包成结构体会让调用点更啰嗦
pub async fn record(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    model: &str,
    day: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
    thinking_tokens: i64,
    cached_tokens: i64,
) -> Result<()> {
    if day.trim().is_empty() {
        return Err(anyhow!("day must not be empty (YYYY-MM-DD)"));
    }
    let month: String = day.chars().take(7).collect();
    if month.len() != 7 {
        return Err(anyhow!("day must look like YYYY-MM-DD: {day}"));
    }
    let (p, c, t, ca) = (
        prompt_tokens.max(0),
        completion_tokens.max(0),
        thinking_tokens.max(0),
        cached_tokens.max(0),
    );
    upsert_usage(
        store,
        "usage_daily",
        "day",
        &[tenant_id, user_id, model, day],
        (p, c, t, ca),
        true,
    )
    .await?;
    upsert_usage(
        store,
        "usage_monthly",
        "month",
        &[tenant_id, user_id, model, &month],
        (p, c, t, ca),
        false,
    )
    .await
}

/// 累加到 `table`：SQLite/Postgres 用 `ON CONFLICT`，MySQL 用 `ON DUPLICATE KEY`。
async fn upsert_usage(
    store: &AdminStore,
    table: &str,
    period_column: &str,
    keys: &[&str],
    tokens: (i64, i64, i64, i64),
    with_id: bool,
) -> Result<()> {
    let (p, c, t, ca) = tokens;
    let increment = "prompt_tokens = prompt_tokens + ?, \
                     completion_tokens = completion_tokens + ?, \
                     thinking_tokens = thinking_tokens + ?, \
                     cached_tokens = cached_tokens + ?";
    // `usage_daily` 有自增主键 `id`，月表没有（复合唯一键就是主键）。
    let (id_col, id_slot) = if with_id { ("id, ", "?, ") } else { ("", "") };
    let raw = match store.backend() {
        crate::db::Backend::MySql => format!(
            "INSERT INTO {table} ({id_col}tenant_id, user_id, model, {period_column},
                                  prompt_tokens, completion_tokens, thinking_tokens, cached_tokens)
             VALUES ({id_slot}?, ?, ?, ?, ?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE {increment}"
        ),
        _ => format!(
            "INSERT INTO {table} ({id_col}tenant_id, user_id, model, {period_column},
                                  prompt_tokens, completion_tokens, thinking_tokens, cached_tokens)
             VALUES ({id_slot}?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (tenant_id, user_id, model, {period_column}) DO UPDATE SET {increment}"
        ),
    };
    let sql = store.sql(&raw);
    // 占位符：插入值（带 id 时 9 个、不带 8 个）+ 4 个累加值。
    let mut query = sqlx::query::<Any>(&sql);
    if with_id {
        query = query.bind(random_id());
    }
    query
        .bind(keys[0])
        .bind(keys[1])
        .bind(keys[2])
        .bind(keys[3])
        .bind(p)
        .bind(c)
        .bind(t)
        .bind(ca)
        .bind(p)
        .bind(c)
        .bind(t)
        .bind(ca)
        .execute(store.pool())
        .await?;
    Ok(())
}

pub async fn list(
    store: &AdminStore,
    tenant_id: &str,
    from_day: &str,
    to_day: &str,
    user_id: Option<&str>,
) -> Result<Vec<UsageRow>> {
    let (sql, with_user) = match user_id {
        Some(_) => (
            store.sql(
                "SELECT user_id, model, day, prompt_tokens, completion_tokens, thinking_tokens, cached_tokens FROM usage_daily
                 WHERE tenant_id = ? AND day >= ? AND day <= ? AND user_id = ?
                 ORDER BY day, user_id, model",
            ),
            true,
        ),
        None => (
            store.sql(
                "SELECT user_id, model, day, prompt_tokens, completion_tokens, thinking_tokens, cached_tokens FROM usage_daily
                 WHERE tenant_id = ? AND day >= ? AND day <= ?
                 ORDER BY day, user_id, model",
            ),
            false,
        ),
    };
    let mut query = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(from_day)
        .bind(to_day);
    if with_user {
        query = query.bind(user_id.unwrap_or_default());
    }
    let rows = query.fetch_all(store.pool()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(UsageRow {
            user_id: row.try_get(0)?,
            model: row.try_get(1)?,
            day: row.try_get(2)?,
            prompt_tokens: row.try_get(3)?,
            completion_tokens: row.try_get(4)?,
            thinking_tokens: row.try_get(5)?,
            cached_tokens: row.try_get(6)?,
        });
    }
    Ok(out)
}

/// 区间内的总量。走 SQL 聚合，不把明细搬进内存。
pub async fn totals(
    store: &AdminStore,
    tenant_id: &str,
    from_day: &str,
    to_day: &str,
    user_id: Option<&str>,
) -> Result<UsageAgg> {
    let sql = store.sql(&format!(
        "SELECT COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0),
                COALESCE(SUM(thinking_tokens), 0), COALESCE(SUM(cached_tokens), 0)
           FROM usage_daily
          WHERE tenant_id = ? AND day >= ? AND day <= ?{}",
        user_filter_sql(user_id)
    ));
    let mut query = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(from_day)
        .bind(to_day);
    if let Some(uid) = user_id {
        query = query.bind(uid);
    }
    let row = query.fetch_one(store.pool()).await?;
    Ok(UsageAgg {
        key: String::new(),
        prompt: row.try_get(0)?,
        completion: row.try_get(1)?,
        thinking: row.try_get(2)?,
        cached: row.try_get(3)?,
    })
}

/// 按维度聚合，**在库里 GROUP BY**。
///
/// 之前是把区间内所有明细行取回进程再内存聚合（O(行数 × 组数)），
/// 用户 × 模型 × 天数一多就顶不住。现在只返回分组结果（几十行）。
pub async fn aggregate_by(
    store: &AdminStore,
    tenant_id: &str,
    from_day: &str,
    to_day: &str,
    user_id: Option<&str>,
    by: GroupBy,
) -> Result<Vec<UsageAgg>> {
    aggregate_by_period(
        store, "usage_daily", "day", tenant_id, from_day, to_day, user_id, by,
    )
    .await
}

/// 在任意「周期表」上聚合：日表 `usage_daily(day)` 或月表 `usage_monthly(month)`。
#[allow(clippy::too_many_arguments)] // 表名/周期列/区间/维度，拆开反而更难读
async fn aggregate_by_period(
    store: &AdminStore,
    table: &str,
    period_column: &str,
    tenant_id: &str,
    from_period: &str,
    to_period: &str,
    user_id: Option<&str>,
    by: GroupBy,
) -> Result<Vec<UsageAgg>> {
    let column = match by {
        GroupBy::User => "user_id",
        GroupBy::Model => "model",
        GroupBy::Day => period_column,
    };
    let sql = store.sql(&format!(
        "SELECT {column} AS k,
                COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0),
                COALESCE(SUM(thinking_tokens), 0), COALESCE(SUM(cached_tokens), 0)
           FROM {table}
          WHERE tenant_id = ? AND {period_column} >= ? AND {period_column} <= ?{}
          GROUP BY {column}
          ORDER BY SUM(prompt_tokens + completion_tokens) DESC, k",
        user_filter_sql(user_id)
    ));
    let mut query = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(from_period)
        .bind(to_period);
    if let Some(uid) = user_id {
        query = query.bind(uid);
    }
    let rows = query.fetch_all(store.pool()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(UsageAgg {
            key: row.try_get(0)?,
            prompt: row.try_get(1)?,
            completion: row.try_get(2)?,
            thinking: row.try_get(3)?,
            cached: row.try_get(4)?,
        });
    }
    Ok(out)
}

/// 区间聚合的统一入口：**整月对齐且跨 ≥2 个月**时走月表，否则走日表。
///
/// 月表行数是日表的 1/28~1/31，年度报表用它；但只覆盖整月区间——
/// 半月的区间如果混用两张表会漏掉未满月的部分，所以宁可用日表。
pub async fn aggregate_range(
    store: &AdminStore,
    tenant_id: &str,
    from_day: &str,
    to_day: &str,
    user_id: Option<&str>,
    by: GroupBy,
) -> Result<Vec<UsageAgg>> {
    match monthly_range(from_day, to_day) {
        Some((from_month, to_month)) if month_span(&from_month, &to_month) >= 2 => {
            aggregate_by_period(
                store,
                "usage_monthly",
                "month",
                tenant_id,
                &from_month,
                &to_month,
                user_id,
                by,
            )
            .await
        }
        _ => {
            aggregate_by(store, tenant_id, from_day, to_day, user_id, by).await
        }
    }
}

/// 区间是否覆盖整月：`from` 是某月 1 号，`to` 是某月最后一天。
/// 返回 `(起始月, 结束月)`（`YYYY-MM`）。
fn monthly_range(from_day: &str, to_day: &str) -> Option<(String, String)> {
    let from = from_day.trim();
    let to = to_day.trim();
    if from.len() != 10 || to.len() != 10 || !from.ends_with("-01") {
        return None;
    }
    let to_month = &to[..7];
    let expected_last = days_in_month(to_month)?;
    if to[8..10].parse::<u32>().ok()? != expected_last {
        return None;
    }
    Some((from[..7].to_string(), to_month.to_string()))
}

fn days_in_month(month: &str) -> Option<u32> {
    let year: u32 = month.get(..4)?.parse().ok()?;
    let mon: u32 = month.get(5..7)?.parse().ok()?;
    let days = match mon {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100))
                || year.is_multiple_of(400);
            if leap {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    Some(days)
}

/// 两个 `YYYY-MM` 之间相差几个月（含两端）。
fn month_span(from: &str, to: &str) -> i64 {
    let parse = |s: &str| -> Option<(i64, i64)> {
        Some((s.get(..4)?.parse().ok()?, s.get(5..7)?.parse().ok()?))
    };
    let (Some((fy, fm)), Some((ty, tm))) = (parse(from), parse(to)) else {
        return 0;
    };
    (ty * 12 + tm) - (fy * 12 + fm) + 1
}

fn user_filter_sql(user_id: Option<&str>) -> &'static str {
    if user_id.is_some() {
        " AND user_id = ?"
    } else {
        ""
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quota {
    pub user_id: String,
    pub monthly_token_limit: Option<i64>,
    pub over_action: String,
}

pub async fn get_quota(store: &AdminStore, user_id: &str) -> Result<Option<Quota>> {
    let sql = store.sql(
        "SELECT user_id, monthly_token_limit, over_action FROM quotas WHERE user_id = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(user_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(Quota {
        user_id: row.try_get(0)?,
        monthly_token_limit: row.try_get(1)?,
        over_action: row.try_get(2)?,
    }))
}

pub async fn set_quota(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    monthly_token_limit: Option<i64>,
    over_action: &str,
) -> Result<()> {
    if over_action != OVER_ACTION_WARN && over_action != OVER_ACTION_BLOCK {
        return Err(anyhow!("unknown over_action: {over_action}"));
    }
    let existing = get_quota(store, user_id).await?.is_some();
    if existing {
        let sql = store.sql(
            "UPDATE quotas SET monthly_token_limit = ?, over_action = ?, updated_at = ?
             WHERE user_id = ?",
        );
        sqlx::query::<Any>(&sql)
            .bind(monthly_token_limit)
            .bind(over_action)
            .bind(now_ts())
            .bind(user_id)
            .execute(store.pool())
            .await?;
    } else {
        let sql = store.sql(
            "INSERT INTO quotas (user_id, tenant_id, monthly_token_limit, over_action, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        );
        sqlx::query::<Any>(&sql)
            .bind(user_id)
            .bind(tenant_id)
            .bind(monthly_token_limit)
            .bind(over_action)
            .bind(now_ts())
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaState {
    pub limit: Option<i64>,
    pub used: i64,
    pub over_action: String,
}

impl QuotaState {
    pub fn exceeded(&self) -> bool {
        matches!(self.limit, Some(limit) if self.used >= limit)
    }

    /// 超限且策略是 block 时，必须拦住。
    pub fn blocks(&self) -> bool {
        self.exceeded() && self.over_action == OVER_ACTION_BLOCK
    }
}

/// 当月（`YYYY-MM` 前缀）用量 + 配额。
pub async fn quota_state(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    month_prefix: &str,
) -> Result<QuotaState> {
    let from = format!("{month_prefix}-01");
    let to = format!("{month_prefix}-31");
    let rows = list(store, tenant_id, &from, &to, Some(user_id)).await?;
    let used: i64 = rows.iter().map(|r| r.total()).sum();
    let quota = get_quota(store, user_id).await?;
    Ok(QuotaState {
        limit: quota.as_ref().and_then(|q| q.monthly_token_limit),
        used,
        over_action: quota
            .map(|q| q.over_action)
            .unwrap_or_else(|| OVER_ACTION_WARN.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AdminStore;
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    #[tokio::test]
    async fn record_accumulates_same_key() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-07", 10, 5, 0, 0)
            .await
            .expect("first");
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-07", 3, 2, 0, 0)
            .await
            .expect("second");

        let rows = list(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None)
            .await
            .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].prompt_tokens, 13);
        assert_eq!(rows[0].completion_tokens, 7);
        assert_eq!(rows[0].total(), 20);
    }

    #[tokio::test]
    async fn list_filters_by_day_and_user() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 1, 1, 0, 0)
            .await
            .expect("a");
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-12-01", 1, 1, 0, 0)
            .await
            .expect("b");
        record(&store, DEFAULT_TENANT, "u2", "qwen", "2026-11-02", 1, 1, 0, 0)
            .await
            .expect("c");

        let nov = list(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None)
            .await
            .expect("nov");
        assert_eq!(nov.len(), 2);

        let u1_nov = list(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", Some("u1"))
            .await
            .expect("u1");
        assert_eq!(u1_nov.len(), 1);
    }

    #[tokio::test]
    async fn aggregate_groups_and_sorts() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 10, 10, 0, 0)
            .await
            .expect("a");
        record(&store, DEFAULT_TENANT, "u2", "qwen", "2026-11-01", 5, 5, 0, 0)
            .await
            .expect("b");
        record(&store, DEFAULT_TENANT, "u1", "deepseek", "2026-11-02", 40, 0, 0, 0)
            .await
            .expect("c");

        let by_user = aggregate_by(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None, GroupBy::User)
            .await
            .expect("by user");
        assert_eq!(by_user[0].key, "u1");
        assert_eq!(by_user[0].total(), 60);
        assert_eq!(by_user[1].key, "u2");

        let by_model = aggregate_by(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None, GroupBy::Model)
            .await
            .expect("by model");
        assert_eq!(by_model[0].key, "deepseek");

        let by_day = aggregate_by(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None, GroupBy::Day)
            .await
            .expect("by day");
        assert_eq!(by_day.len(), 2);

        // 带用户过滤：只算 u2 那一条
        let only_u2 = aggregate_by(
            &store,
            DEFAULT_TENANT,
            "2026-11-01",
            "2026-11-30",
            Some("u2"),
            GroupBy::Model,
        )
        .await
        .expect("filtered");
        assert_eq!(only_u2.len(), 1);
        assert_eq!(only_u2[0].total(), 10);

        let totals_row = totals(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None)
            .await
            .expect("totals");
        assert_eq!(totals_row.total(), 70);
    }

    /// upsert：同一 (用户, 模型, 天) 累加，不产生第二行。
    #[tokio::test]
    async fn record_accumulates_without_duplicate_rows() {
        let store = store().await;
        for _ in 0..3 {
            record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-05", 10, 5, 1, 2)
                .await
                .expect("record");
        }
        let rows = list(&store, DEFAULT_TENANT, "2026-11-05", "2026-11-05", Some("u1"))
            .await
            .expect("list");
        assert_eq!(rows.len(), 1, "同一天同一模型只应有一行");
        assert_eq!(rows[0].prompt_tokens, 30);
        assert_eq!(rows[0].completion_tokens, 15);
        assert_eq!(rows[0].thinking_tokens, 3);
        assert_eq!(rows[0].cached_tokens, 6);
    }

    /// 月表与日表口径一致：写同样数据，两边聚合结果必须相同。
    #[tokio::test]
    async fn monthly_table_matches_daily_table() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-09-03", 10, 5, 0, 0)
            .await
            .expect("a");
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-10-04", 20, 7, 1, 0)
            .await
            .expect("b");
        record(&store, DEFAULT_TENANT, "u2", "deepseek", "2026-10-05", 5, 5, 0, 0)
            .await
            .expect("c");

        let daily = aggregate_by(
            &store,
            DEFAULT_TENANT,
            "2026-09-01",
            "2026-10-31",
            None,
            GroupBy::User,
        )
        .await
        .expect("daily");
        let monthly = aggregate_range(
            &store,
            DEFAULT_TENANT,
            "2026-09-01",
            "2026-10-31",
            None,
            GroupBy::User,
        )
        .await
        .expect("monthly");
        assert_eq!(daily, monthly, "整月区间走月表，结果必须和日表一致");

        // 非整月区间回落日表
        let partial = aggregate_range(
            &store,
            DEFAULT_TENANT,
            "2026-09-15",
            "2026-10-31",
            None,
            GroupBy::User,
        )
        .await
        .expect("partial");
        assert_eq!(
            partial.iter().map(|a| a.total()).sum::<i64>(),
            37,
            "9/15 起算只剩 10 月两笔：u1 27 + u2 10"
        );
    }

    #[test]
    fn month_helpers() {
        assert_eq!(days_in_month("2026-02"), Some(28));
        assert_eq!(days_in_month("2024-02"), Some(29));
        assert_eq!(days_in_month("2026-11"), Some(30));
        assert!(monthly_range("2026-01-01", "2026-03-31").is_some());
        assert!(monthly_range("2026-01-01", "2026-03-30").is_none());
        assert!(monthly_range("2026-01-02", "2026-03-31").is_none());
        assert_eq!(month_span("2026-01", "2026-12"), 12);
        assert_eq!(month_span("2025-11", "2026-01"), 3);
    }

    #[tokio::test]
    async fn quota_blocks_only_when_action_is_block() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 60, 40, 0, 0)
            .await
            .expect("usage");

        set_quota(&store, DEFAULT_TENANT, "u1", Some(50), OVER_ACTION_WARN)
            .await
            .expect("warn quota");
        let state = quota_state(&store, DEFAULT_TENANT, "u1", "2026-11")
            .await
            .expect("state");
        assert_eq!(state.used, 100);
        assert!(state.exceeded());
        assert!(!state.blocks(), "warn 只提示不拦");

        set_quota(&store, DEFAULT_TENANT, "u1", Some(50), OVER_ACTION_BLOCK)
            .await
            .expect("block quota");
        let state = quota_state(&store, DEFAULT_TENANT, "u1", "2026-11")
            .await
            .expect("state");
        assert!(state.blocks());
    }

    #[tokio::test]
    async fn quota_without_limit_never_blocks() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 1000, 0, 0, 0)
            .await
            .expect("usage");
        let state = quota_state(&store, DEFAULT_TENANT, "u1", "2026-11")
            .await
            .expect("state");
        assert_eq!(state.limit, None);
        assert!(!state.exceeded());
        assert!(!state.blocks());
    }

    #[tokio::test]
    async fn unknown_over_action_rejected() {
        let store = store().await;
        assert!(set_quota(&store, DEFAULT_TENANT, "u1", Some(1), "explode")
            .await
            .is_err());
    }
}
