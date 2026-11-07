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
}

impl UsageRow {
    pub fn total(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    User,
    Model,
    Day,
}

/// 累加一次上报。
pub async fn record(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    model: &str,
    day: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
) -> Result<()> {
    if day.trim().is_empty() {
        return Err(anyhow!("day must not be empty (YYYY-MM-DD)"));
    }
    let find_sql = store.sql(
        "SELECT id FROM usage_daily WHERE tenant_id = ? AND user_id = ? AND model = ? AND day = ?",
    );
    let existing = sqlx::query::<Any>(&find_sql)
        .bind(tenant_id)
        .bind(user_id)
        .bind(model)
        .bind(day)
        .fetch_optional(store.pool())
        .await?;

    match existing {
        Some(row) => {
            let id: String = row.try_get(0)?;
            let update_sql = store.sql(
                "UPDATE usage_daily
                 SET prompt_tokens = prompt_tokens + ?, completion_tokens = completion_tokens + ?
                 WHERE id = ?",
            );
            sqlx::query::<Any>(&update_sql)
                .bind(prompt_tokens.max(0))
                .bind(completion_tokens.max(0))
                .bind(id)
                .execute(store.pool())
                .await?;
        }
        None => {
            let insert_sql = store.sql(
                "INSERT INTO usage_daily
                 (id, tenant_id, user_id, model, day, prompt_tokens, completion_tokens)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            );
            sqlx::query::<Any>(&insert_sql)
                .bind(random_id())
                .bind(tenant_id)
                .bind(user_id)
                .bind(model)
                .bind(day)
                .bind(prompt_tokens.max(0))
                .bind(completion_tokens.max(0))
                .execute(store.pool())
                .await?;
        }
    }
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
                "SELECT user_id, model, day, prompt_tokens, completion_tokens FROM usage_daily
                 WHERE tenant_id = ? AND day >= ? AND day <= ? AND user_id = ?
                 ORDER BY day, user_id, model",
            ),
            true,
        ),
        None => (
            store.sql(
                "SELECT user_id, model, day, prompt_tokens, completion_tokens FROM usage_daily
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
        });
    }
    Ok(out)
}

/// 聚合为 `(key, prompt, completion)`，按 token 总数降序。
pub fn aggregate(rows: &[UsageRow], by: GroupBy) -> Vec<(String, i64, i64)> {
    let mut map: Vec<(String, i64, i64)> = Vec::new();
    for row in rows {
        let key = match by {
            GroupBy::User => row.user_id.clone(),
            GroupBy::Model => row.model.clone(),
            GroupBy::Day => row.day.clone(),
        };
        match map.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, prompt, completion)) => {
                *prompt += row.prompt_tokens;
                *completion += row.completion_tokens;
            }
            None => map.push((key, row.prompt_tokens, row.completion_tokens)),
        }
    }
    map.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)).then_with(|| a.0.cmp(&b.0)));
    map
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
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-07", 10, 5)
            .await
            .expect("first");
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-07", 3, 2)
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
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 1, 1)
            .await
            .expect("a");
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-12-01", 1, 1)
            .await
            .expect("b");
        record(&store, DEFAULT_TENANT, "u2", "qwen", "2026-11-02", 1, 1)
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
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 10, 10)
            .await
            .expect("a");
        record(&store, DEFAULT_TENANT, "u2", "qwen", "2026-11-01", 5, 5)
            .await
            .expect("b");
        record(&store, DEFAULT_TENANT, "u1", "deepseek", "2026-11-02", 40, 0)
            .await
            .expect("c");

        let rows = list(&store, DEFAULT_TENANT, "2026-11-01", "2026-11-30", None)
            .await
            .expect("list");

        let by_user = aggregate(&rows, GroupBy::User);
        assert_eq!(by_user[0].0, "u1");
        assert_eq!(by_user[0].1 + by_user[0].2, 60);
        assert_eq!(by_user[1].0, "u2");

        let by_model = aggregate(&rows, GroupBy::Model);
        assert_eq!(by_model[0].0, "deepseek");

        let by_day = aggregate(&rows, GroupBy::Day);
        assert_eq!(by_day.len(), 2);
    }

    #[tokio::test]
    async fn quota_blocks_only_when_action_is_block() {
        let store = store().await;
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 60, 40)
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
        record(&store, DEFAULT_TENANT, "u1", "qwen", "2026-11-01", 1000, 0)
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
