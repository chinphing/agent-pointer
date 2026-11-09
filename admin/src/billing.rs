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

/// 模型费率：以基准模型 1.0 为参照的倍数，整数存（ratio × 1e6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ratio {
    pub ratio_micro: i64,
}

impl Ratio {
    pub fn from_ratio(ratio: f64) -> Self {
        Self {
            ratio_micro: (ratio.max(0.0) * 1_000_000.0).round() as i64,
        }
    }

    pub fn as_f64(&self) -> f64 {
        self.ratio_micro as f64 / 1_000_000.0
    }

    /// 一次调用的费用（微元）。
    ///
    /// 对齐官网：`ceil(raw × 基准价 × ratio ÷ 2²⁰)`，输入输出不分价。
    /// `base_micro_per_mb` 是基准价（微元 / MB，默认 1 元 = 1_000_000）。
    pub fn cost_micro(&self, total_tokens: i64, base_micro_per_mb: i64) -> i64 {
        let t = total_tokens.max(0) as i128;
        let r = self.ratio_micro.max(0) as i128;
        let b = base_micro_per_mb.max(0) as i128;
        let num = t * r * b;
        let den = 1_000_000i128 * 1_048_576i128; // ratio 缩放 × 2²⁰
        ((num + den - 1) / den) as i64
    }
}

/// 基准价默认值：1 元 / MB（官网 `DEFAULT_LLM_BASE_YUAN_PER_MB`）。
pub const DEFAULT_BASE_MICRO_PER_MB: i64 = MICRO_PER_YUAN;

const META_BASE_PRICE: &str = "llm_base_micro_per_mb";

/// 读全局基准价（微元 / MB）。
pub async fn base_price_micro_per_mb(store: &AdminStore) -> Result<i64> {
    let row = store
        .fetch_optional(&format!(
            "SELECT meta_value FROM meta WHERE meta_key = '{META_BASE_PRICE}'"
        ))
        .await?;
    let Some(row) = row else {
        return Ok(DEFAULT_BASE_MICRO_PER_MB);
    };
    let raw: String = row.try_get(0)?;
    Ok(raw
        .trim()
        .parse::<i64>()
        .unwrap_or(DEFAULT_BASE_MICRO_PER_MB))
}

/// 写全局基准价。
pub async fn set_base_price_micro_per_mb(store: &AdminStore, value: i64) -> Result<()> {
    if value < 0 {
        return Err(anyhow!("基准价必须是非负数"));
    }
    let sql = store.sql(&format!(
        "UPDATE meta SET meta_value = ? WHERE meta_key = '{META_BASE_PRICE}'"
    ));
    let updated = sqlx::query::<Any>(&sql)
        .bind(value.to_string())
        .execute(store.pool())
        .await?
        .rows_affected();
    if updated == 0 {
        let sql = store.sql("INSERT INTO meta (meta_key, meta_value) VALUES (?, ?)");
        sqlx::query::<Any>(&sql)
            .bind(META_BASE_PRICE)
            .bind(value.to_string())
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

/// 取某模型的单价：**只认显式配置**。没配价就是没配价，不猜、不兜底。
pub async fn price_for(store: &AdminStore, tenant_id: &str, model: &str) -> Result<Option<Ratio>> {
    let sql = store.sql(
        "SELECT ratio_micro FROM model_prices WHERE tenant_id = ? AND model = ?",
    );
    let row = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(model)
        .fetch_optional(store.pool())
        .await?;
    Ok(match row {
        Some(row) => Some(Ratio {
            ratio_micro: row.try_get(0)?,
        }),
        None => None,
    })
}

/// 写单价（管理端）。
pub async fn set_price(
    store: &AdminStore,
    tenant_id: &str,
    model: &str,
    ratio: Ratio,
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
                SET ratio_micro = ?, updated_at = ?
              WHERE tenant_id = ? AND model = ?",
        );
        sqlx::query::<Any>(&sql)
            .bind(ratio.ratio_micro)
            .bind(now_ts())
            .bind(tenant_id)
            .bind(model)
            .execute(store.pool())
            .await?;
    } else {
        let sql = store.sql(
            "INSERT INTO model_prices
                (tenant_id, model, prompt_micro_per_million, completion_micro_per_million,
                 micro_per_million, ratio_micro, updated_at)
             VALUES (?, ?, 0, 0, 0, ?, ?)",
        );
        // 老的三列（两档价 + 换算价）保留但不再读；权威只有 ratio_micro。
        sqlx::query::<Any>(&sql)
            .bind(tenant_id)
            .bind(model)
            .bind(ratio.ratio_micro)
            .bind(now_ts())
            .execute(store.pool())
            .await?;
    }
    Ok(())
}

pub async fn list_prices(store: &AdminStore, tenant_id: &str) -> Result<Vec<(String, Ratio)>> {
    let sql = store.sql(
        "SELECT model, ratio_micro FROM model_prices WHERE tenant_id = ? ORDER BY model",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((
            row.try_get(0)?,
            Ratio {
                ratio_micro: row.try_get(1)?,
            },
        ));
    }
    Ok(out)
}

/// 删除某模型的显式单价：删掉后该模型回到兜底价。
pub async fn delete_price(store: &AdminStore, tenant_id: &str, model: &str) -> Result<bool> {
    if model.trim().is_empty() {
        return Err(anyhow!("model must not be empty"));
    }
    let sql = store.sql("DELETE FROM model_prices WHERE tenant_id = ? AND model = ?");
    let res = sqlx::query::<Any>(&sql)
        .bind(tenant_id)
        .bind(model)
        .execute(store.pool())
        .await?;
    Ok(res.rows_affected() > 0)
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
/// 一次上报的 token 账：`total` 用于计费，细分只落账本给人看。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageTokens {
    pub total: i64,
    pub prompt: i64,
    pub completion: i64,
}

pub async fn charge(
    store: &AdminStore,
    tenant_id: &str,
    user_id: &str,
    model: &str,
    tokens: UsageTokens,
) -> Result<(i64, i64)> {
    let ratio = price_for(store, tenant_id, model).await?.ok_or_else(|| {
        anyhow!(
            "model_price_missing: 模型 {model} 没有配置费率，已拒绝计费（先在管理端「计费」里填费率）"
        )
    })?;
    let base = base_price_micro_per_mb(store).await?;
    let cost = ratio.cost_micro(tokens.total, base);
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
        tokens.prompt,
        tokens.completion,
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
            Ratio::from_ratio(1.0),
        )
        .await
        .expect("price");
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");

        // 200 万 token × 基准价 1 元/MB × ratio 1.0 = ceil(2e6 × 2²⁰⁻¹) ≈ 1.907349 元
        let (cost, balance) = charge(&store, DEFAULT_TENANT, "u1", "test-model", UsageTokens { total: 2_000_000, prompt: 2_000_000, completion: 0 })
            .await
            .expect("charge");
        assert_eq!(cost, 1_907_349, "200 万 token × 1 元/MB × ratio 1.0");
        assert_eq!(yuan_string(balance), "8.09");
    }

    /// 余额用光后 exhausted 判定生效（balance <= 0）。
    #[tokio::test]
    async fn balance_can_go_negative_and_blocks() {
        let store = store().await;
        set_price(
            &store,
            DEFAULT_TENANT,
            "expensive",
            Ratio::from_ratio(100.0),
        )
        .await
        .expect("price");
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        // 100 万 token = 100 元，比 10 元额度多
        let (_, balance) = charge(&store, DEFAULT_TENANT, "u1", "expensive", UsageTokens { total: 1_000_000, prompt: 1_000_000, completion: 0 })
            .await
            .expect("charge");
        assert!(balance < 0, "应透支到负数，实际 {balance}");
        // 100 万 token × ratio 100 = 190.73 元，10 元额度撑不住
        assert_eq!(yuan_string(balance), "-85.37");
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

    /// 没配单价的模型必须**拒绝计费**，不再按名字猜一个兜底价。
    #[tokio::test]
    async fn unpriced_model_is_rejected() {
        let store = store().await;
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        let err = charge(&store, DEFAULT_TENANT, "u1", "some-new-flash", UsageTokens { total: 1_000_000, prompt: 1_000_000, completion: 0 })
            .await
            .expect_err("没配价必须报错");
        assert!(
            err.to_string().contains("model_price_missing"),
            "错误里要点明是缺单价：{err}"
        );
    }

    /// 配了价就按配置扣，且钱真的从余额里走。
    #[tokio::test]
    async fn configured_price_is_charged() {
        let store = store().await;
        ensure_wallet(&store, DEFAULT_TENANT, "u1").await.expect("wallet");
        set_price(
            &store,
            DEFAULT_TENANT,
            "priced-model",
            Ratio::from_ratio(1.0),
        )
        .await
        .expect("set price");
        let (cost, _) = charge(&store, DEFAULT_TENANT, "u1", "priced-model", UsageTokens { total: 1_000_000, prompt: 1_000_000, completion: 0 })
            .await
            .expect("charge");
        // ceil(1e6 × 1e6 × 1e6 ÷ (1e6 × 2²⁰)) = ceil(953674.32) = 953675 微元
        assert_eq!(cost, 953_675, "100 万 token × 1 元/MB × ratio 1.0");
    }

    #[test]
    fn cost_uses_single_rate_on_total_tokens() {
        // 口径对齐官网：单费率 × 总 token，输入输出不分价。
        let r = Ratio::from_ratio(1.0);
        let base = DEFAULT_BASE_MICRO_PER_MB; // 1 元 / MB
        // 200 万 token
        assert_eq!(r.cost_micro(2_000_000, base), 1_907_349);
        // 1000 token
        assert_eq!(r.cost_micro(1_000, base), 954);
        // 不足 1 微元向上取整（官网也是 ceil）
        assert_eq!(r.cost_micro(1, base), 1);
        // 负数/0 不产生负费用
        assert_eq!(r.cost_micro(0, base), 0);
        assert_eq!(r.cost_micro(-5, base), 0);
        // 基准价翻倍 → 费用翻倍
        assert_eq!(r.cost_micro(2_000_000, base * 2), 3_814_698);
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

    /// 兜底价已删除：没配价就是 `None`，绝不按模型名猜。
    #[tokio::test]
    async fn unknown_model_has_no_price() {
        let store = store().await;
        assert!(
            price_for(&store, DEFAULT_TENANT, "qwen3.8-max")
                .await
                .expect("query")
                .is_none(),
            "没配价的模型不应返回任何价格"
        );
    }
}
