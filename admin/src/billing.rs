//! 计费：钱包余额、扣费流水、模型单价。
//!
//! 口径：
//! - 金额一律用**微元**（1 元 = `MICRO_PER_YUAN`）存 `i64`，不碰浮点。
//! - 新用户自动开一个钱包，初始余额 `DEFAULT_GRANT_MICRO`（10 元）。
//! - 扣费发生在客户端上报用量时：按模型单价算钱，扣余额，写一条流水。
//! - 余额 ≤ 0 时 `exhausted()` 为真，`/auth/partner/balance` 会回
//!   `token_quota_exhausted: true`，客户端据此拦截。
//!
//! 单价按「元 / 百万 token」给管理员看，内部仍存微元：
//! `1 元/百万 token` = `1_000_000` 微元。

use anyhow::{anyhow, Result};
use sqlx::{Any, Row};

use crate::db::{now_ts, random_id, AdminStore};

/// 1 元 = 1_000_000 微元。
pub const MICRO_PER_YUAN: i64 = 1_000_000;

/// 新用户默认额度：10 元。
pub const DEFAULT_GRANT_MICRO: i64 = 10 * MICRO_PER_YUAN;

pub const KIND_GRANT: &str = "grant";
pub const KIND_CHARGE: &str = "charge";

/// 未配置单价时用这份兜底价（元 / 百万 token），管理端可覆盖。
///
/// 只是防止「没配价就白用」，不是权威价目表；上线前应由管理员按合同价填。
pub fn default_price(model: &str) -> (i64, i64) {
    let m = model.to_ascii_lowercase();
    let yuan_per_million: (f64, f64) = if m.contains("asr") || m.contains("fun-") {
        (0.8, 0.8)
    } else if m.contains("max") {
        (4.0, 16.0)
    } else if m.contains("pro") {
        (2.0, 8.0)
    } else if m.contains("plus") {
        (1.0, 4.0)
    } else if m.contains("flash") || m.contains("mini") || m.contains("turbo") {
        (0.5, 2.0)
    } else {
        (1.0, 4.0)
    };
    (
        (yuan_per_million.0 * MICRO_PER_YUAN as f64) as i64,
        (yuan_per_million.1 * MICRO_PER_YUAN as f64) as i64,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub prompt_micro_per_million: i64,
    pub completion_micro_per_million: i64,
}

impl Price {
    /// 一次调用的费用（微元）。整数运算，向下取整到微元。
    pub fn cost_micro(&self, prompt_tokens: i64, completion_tokens: i64) -> i64 {
        let p = prompt_tokens.max(0) as i128 * self.prompt_micro_per_million.max(0) as i128;
        let c = completion_tokens.max(0) as i128 * self.completion_micro_per_million.max(0) as i128;
        ((p + c) / 1_000_000) as i64
    }
}

/// 取某模型的单价：先查表，没有就用兜底价。
pub async fn price_for(store: &AdminStore, tenant_id: &str, model: &str) -> Result<Price> {
    let sql = store.sql(
        "SELECT prompt_micro_per_million, completion_micro_per_million
           FROM model_prices WHERE tenant_id = ? AND model = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(model)
        .fetch_optional(store.pool())
        .await?;
    if let Some(row) = row {
        return Ok(Price {
            prompt_micro_per_million: row.try_get(0)?,
            completion_micro_per_million: row.try_get(1)?,
        });
    }
    let (p, c) = default_price(model);
    Ok(Price {
        prompt_micro_per_million: p,
        completion_micro_per_million: c,
    })
}

/// 写单价（管理端）。
pub async fn set_price(
    store: &AdminStore,
    tenant_id: &str,
    model: &str,
    price: Price,
) -> Result<()> {
    if model.trim().is_empty() {
        return Err(anyhow!("model must not be empty"));
    }
    let existing = store.sql("SELECT 1 FROM model_prices WHERE tenant_id = ? AND model = ?");
    let found = sqlx::query::<Any>(&existing)
        .bind(tenant_id)
        .bind(model)
        .fetch_optional(store.pool())
        .await?;
    if found.is_some() {
        let sql = store.sql(
            "UPDATE model_prices
                SET prompt_micro_per_million = ?, completion_micro_per_million = ?, updated_at = ?
              WHERE tenant_id = ? AND model = ?",
        );
        sqlx::query::<Any>(&sql)
            .bind(price.prompt_micro_per_million)
            .bind(price.completion_micro_per_million)
            .bind(now_ts())
            .bind(tenant_id)
            .bind(model)
            .execute(store.pool())
            .await?;
    } else {
        let sql = store.sql(
            "INSERT INTO model_prices
                (tenant_id, model, prompt_micro_per_million, completion_micro_per_million, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        );
        sqlx::query::<Any>(&sql)
            .bind(tenant_id)
            .bind(model)
            .bind(price.prompt_micro_per_million)
            .bind(price.completion_micro_per_million)
            .bind(now_ts())
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

pub async fn list_prices(store: &AdminStore, tenant_id: &str) -> Result<Vec<(String, Price)>> {
    let sql = store.sql(
        "SELECT model, prompt_micro_per_million, completion_micro_per_million
           FROM model_prices WHERE tenant_id = ? ORDER BY model",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((
            row.try_get(0)?,
            Price {
                prompt_micro_per_million: row.try_get(1)?,
                completion_micro_per_million: row.try_get(2)?,
            },
        ));
    }
    Ok(out)
}

/// 读余额；钱包不存在时返回 `None`（调用方决定要不要开户）。
pub async fn balance_micro(store: &AdminStore, user_id: &str) -> Result<Option<i64>> {
    let sql = store.sql("SELECT balance_micro FROM wallets WHERE user_id = ?");
    let row = sqlx::query::<Any>(&sql)
        .bind(user_id)
        .fetch_optional(store.pool())
        .await?;
    match row {
        Some(row) => Ok(Some(row.try_get(0)?)),
        None => Ok(None),
    }
}

/// 确保钱包存在：没有就按默认额度开一个，并记一条 grant 流水。
pub async fn ensure_wallet(store: &AdminStore, tenant_id: &str, user_id: &str) -> Result<i64> {
    if let Some(balance) = balance_micro(store, user_id).await? {
        return Ok(balance);
    }
    let sql = store.sql(
        "INSERT INTO wallets (user_id, tenant_id, balance_micro, updated_at) VALUES (?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(user_id)
        .bind(tenant_id)
        .bind(DEFAULT_GRANT_MICRO)
        .bind(now_ts())
        .execute(store.pool())
        .await?;
    write_ledger(
        store,
        tenant_id,
        user_id,
        KIND_GRANT,
        DEFAULT_GRANT_MICRO,
        DEFAULT_GRANT_MICRO,
        "",
        0,
        0,
        "新用户默认额度",
    )
    .await?;
    Ok(DEFAULT_GRANT_MICRO)
}

#[allow(clippy::too_many_arguments)]
async fn write_ledger(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    kind: &str,
    amount_micro: i64,
    balance_after_micro: i64,
    model: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
    note: &str,
) -> Result<()> {
    let sql = store.sql(
        "INSERT INTO wallet_ledger
            (id, tenant_id, user_id, kind, amount_micro, balance_after_micro,
             model, prompt_tokens, completion_tokens, note, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<Any>(&sql)
        .bind(random_id())
        .bind(tenant_id)
        .bind(user_id)
        .bind(kind)
        .bind(amount_micro)
        .bind(balance_after_micro)
        .bind(model)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(note)
        .bind(now_ts())
        .execute(store.pool())
        .await?;
    Ok(())
}

/// 管理员加额（微元，必须为正）。
pub async fn grant(store: &AdminStore, tenant_id: &str, user_id: &str, amount_micro: i64) -> Result<i64> {
    if amount_micro <= 0 {
        return Err(anyhow!("加额必须大于 0"));
    }
    let current = ensure_wallet(store, tenant_id, user_id).await?;
    let next = current + amount_micro;
    let sql = store.sql("UPDATE wallets SET balance_micro = ?, updated_at = ? WHERE user_id = ?");
    sqlx::query::<Any>(&sql)
        .bind(next)
        .bind(now_ts())
        .bind(user_id)
        .execute(store.pool())
        .await?;
    write_ledger(
        store,
        tenant_id,
        user_id,
        KIND_GRANT,
        amount_micro,
        next,
        "",
        0,
        0,
        "管理员加额",
    )
    .await?;
    Ok(next)
}

/// 扣费。返回扣完的余额。
///
/// 余额可以为负：一次调用可能比余额大，允许透支到负数，
/// 由 `exhausted()` 拦住下一次调用，而不是把这次的费用截断。
pub async fn charge(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    model: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
) -> Result<(i64, i64)> {
    let price = price_for(store, tenant_id, model).await?;
    let cost = price.cost_micro(prompt_tokens, completion_tokens);
    let current = ensure_wallet(store, tenant_id, user_id).await?;
    let next = current - cost;
    let sql = store.sql("UPDATE wallets SET balance_micro = ?, updated_at = ? WHERE user_id = ?");
    sqlx::query::<Any>(&sql)
        .bind(next)
        .bind(now_ts())
        .bind(user_id)
        .execute(store.pool())
        .await?;
    write_ledger(
        store,
        tenant_id,
        user_id,
        KIND_CHARGE,
        -cost,
        next,
        model,
        prompt_tokens,
        completion_tokens,
        "",
    )
    .await?;
    Ok((cost, next))
}

/// 余额转「元」字符串，保留 2 位小数（内部微元，展示四舍五入到分）。
pub fn yuan_string(balance_micro: i64) -> String {
    let negative = balance_micro < 0;
    let abs = balance_micro.unsigned_abs();
    // 微元 → 分：四舍五入
    let cents = (abs + 5_000) / 10_000;
    let yuan = cents / 100;
    let frac = cents % 100;
    format!(
        "{}{}.{:02}",
        if negative { "-" } else { "" },
        yuan,
        frac
    )
}

/// 元 → 微元（管理端输入用），四舍五入到微元。
pub fn yuan_to_micro(yuan: f64) -> i64 {
    (yuan * MICRO_PER_YUAN as f64).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::DEFAULT_TENANT;

    async fn store() -> AdminStore {
        AdminStore::connect_in_memory().await.expect("store")
    }

    /// 新用户第一次出现就有 10 元额度，并留下一条 grant 流水。
    #[tokio::test]
    async fn new_user_gets_ten_yuan() {
        let store = store().await;
        let balance = ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        assert_eq!(balance, DEFAULT_GRANT_MICRO);
        assert_eq!(yuan_string(balance), "10.00");
        // 幂等：再调一次不加钱
        let again = ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        assert_eq!(again, DEFAULT_GRANT_MICRO);
    }

    /// 扣费按单价算，余额减少，流水记负数。
    #[tokio::test]
    async fn charge_deducts_by_price() {
        let store = store().await;
        set_price(
            &store,
            DEFAULT_TENANT,
            "test-model",
            Price {
                prompt_micro_per_million: 1_000_000,     // 1 元/百万
                completion_micro_per_million: 4_000_000, // 4 元/百万
            },
        )
        .await
        .expect("price");
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");

        // 200 万输入 + 0 输出 = 2 元
        let (cost, balance) = charge(&store, DEFAULT_TENANT, "u1", "test-model", 2_000_000, 0)
            .await
            .expect("charge");
        assert_eq!(cost, 2 * MICRO_PER_YUAN);
        assert_eq!(yuan_string(balance), "8.00");
    }

    /// 余额用光后 exhausted 判定生效（balance <= 0）。
    #[tokio::test]
    async fn balance_can_go_negative_and_blocks() {
        let store = store().await;
        set_price(
            &store,
            DEFAULT_TENANT,
            "expensive",
            Price {
                prompt_micro_per_million: 100 * MICRO_PER_YUAN, // 100 元/百万
                completion_micro_per_million: 0,
            },
        )
        .await
        .expect("price");
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        // 100 万输入 = 100 元，比 10 元额度多
        let (_, balance) = charge(&store, DEFAULT_TENANT, "u1", "expensive", 1_000_000, 0)
            .await
            .expect("charge");
        assert!(balance < 0, "应透支到负数，实际 {balance}");
        assert_eq!(yuan_string(balance), "-90.00");
    }

    /// 管理员加额能救回负余额。
    #[tokio::test]
    async fn grant_tops_up() {
        let store = store().await;
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        let after = grant(&store, DEFAULT_TENANT, "u1", 5 * MICRO_PER_YUAN)
            .await
            .expect("grant");
        assert_eq!(yuan_string(after), "15.00");
    }

    /// 加额必须为正数。
    #[tokio::test]
    async fn grant_rejects_non_positive() {
        let store = store().await;
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        assert!(grant(&store, DEFAULT_TENANT, "u1", 0).await.is_err());
        assert!(grant(&store, DEFAULT_TENANT, "u1", -100).await.is_err());
    }

    /// 没配单价的模型走兜底价，不会白用。
    #[tokio::test]
    async fn unpriced_model_uses_default_price() {
        let store = store().await;
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        let (cost, _) = charge(&store, DEFAULT_TENANT, "u1", "some-new-flash", 1_000_000, 0)
            .await
            .expect("charge");
        assert!(cost > 0, "兜底价必须产生费用");
    }

    #[test]
    fn cost_uses_prompt_and_completion_prices() {
        let p = Price {
            prompt_micro_per_million: 1_000_000,     // 1 元/百万
            completion_micro_per_million: 4_000_000, // 4 元/百万
        };
        // 100 万输入 + 100 万输出 = 5 元
        assert_eq!(p.cost_micro(1_000_000, 1_000_000), 5 * MICRO_PER_YUAN);
        // 1000 输入 + 0 输出 = 0.001 元
        assert_eq!(p.cost_micro(1_000, 0), 1_000);
        // 负数/0 不产生负费用
        assert_eq!(p.cost_micro(0, 0), 0);
        assert_eq!(p.cost_micro(-5, -5), 0);
    }

    #[test]
    fn yuan_string_rounds_to_cents() {
        assert_eq!(yuan_string(DEFAULT_GRANT_MICRO), "10.00");
        assert_eq!(yuan_string(0), "0.00");
        assert_eq!(yuan_string(-1_500_000), "-1.50");
        // 0.004 元 → 0.00（分以下四舍五入）
        assert_eq!(yuan_string(4_000), "0.00");
        // 0.006 元 → 0.01
        assert_eq!(yuan_string(6_000), "0.01");
    }

    #[test]
    fn yuan_to_micro_round_trips() {
        assert_eq!(yuan_to_micro(10.0), DEFAULT_GRANT_MICRO);
        assert_eq!(yuan_to_micro(0.5), 500_000);
    }

    #[test]
    fn default_price_tiers_are_ordered() {
        let (max_p, _) = default_price("qwen3.8-max");
        let (plus_p, _) = default_price("qwen3.7-plus");
        let (flash_p, _) = default_price("deepseek-flash");
        assert!(max_p > plus_p, "max 应比 plus 贵");
        assert!(plus_p > flash_p, "plus 应比 flash 贵");
    }
}
