//! 管理端计费接口：员工钱包（查余额 / 加额）与模型单价。
//!
//! 权限：
//! - 查余额与流水 → `usage:read`
//! - 加额 → `usage:write`（动钱，必须单独授权）
//! - 改单价 → `usage:write`
//!
//! 金额对外一律用「元」（字符串两位小数 / 数值），内部微元不暴露。

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit::{self, AuditInput};
use crate::billing::{self, Price, MICRO_PER_YUAN};
use crate::db::AdminStore;
use crate::http::auth::current_auth;
use crate::http::error::{ApiError, ApiResult};
use crate::http::Shared;
use crate::oauth::partner::bearer_user;

/// 微元 → 「元/百万 token」数值（保留 6 位，够表达 0.000001 元）。
fn micro_per_million_to_yuan(v: i64) -> f64 {
    (v as f64) / (MICRO_PER_YUAN as f64) * 1_000_000.0 / 1_000_000.0
}

pub async fn get_wallet(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:read")?;
    let balance = billing::ensure_wallet(&state.store, &state.config.tenant_id, &user_id).await?;
    let ledger = recent_ledger(&state.store, &user_id, 20).await?;
    Ok(Json(json!({
        "userId": user_id,
        "balanceYuan": billing::yuan_string(balance),
        "balanceMicro": balance,
        "ledger": ledger,
    })))
}

#[derive(Debug, Deserialize)]
pub struct GrantBody {
    /// 加多少元（正数）。
    pub amount_yuan: Option<f64>,
    /// 兼容前端 camelCase / snake_case 两种写法。
    #[serde(default, alias = "amountYuan")]
    pub amount_yuan_alias: Option<f64>,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn grant_wallet(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(body): Json<GrantBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:write")?;
    let yuan = body
        .amount_yuan
        .or(body.amount_yuan_alias)
        .ok_or_else(|| ApiError::BadRequest("缺少 amountYuan".to_string()))?;
    if !yuan.is_finite() || yuan <= 0.0 {
        return Err(ApiError::BadRequest("加额必须大于 0".to_string()));
    }
    let amount_micro = billing::yuan_to_micro(yuan);
    if amount_micro <= 0 {
        return Err(ApiError::BadRequest("加额太小，至少 0.000001 元".to_string()));
    }

    let before = billing::balance_micro(&state.store, &user_id).await?;
    let after = billing::grant(&state.store, &state.config.tenant_id, &user_id, amount_micro)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "wallet.grant",
            target_type: "user",
            target_id: &user_id,
            before_json: before.map(|b| format!(r#"{{"balanceYuan":"{}"}}"#, billing::yuan_string(b))).as_deref(),
            after_json: Some(&format!(
                r#"{{"balanceYuan":"{}","grantYuan":"{:.2}"}}"#,
                billing::yuan_string(after),
                yuan
            )),
        },
    )
    .await?;

    Ok(Json(json!({
        "ok": true,
        "balanceYuan": billing::yuan_string(after),
        "balanceMicro": after,
    })))
}

pub async fn list_prices(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:read")?;
    let rows = billing::list_prices(&state.store, &state.config.tenant_id).await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(model, price)| {
            json!({
                "model": model,
                "promptYuanPerMillion": micro_per_million_to_yuan(price.prompt_micro_per_million),
                "completionYuanPerMillion": micro_per_million_to_yuan(price.completion_micro_per_million),
            })
        })
        .collect();
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct PriceBody {
    pub model: String,
    #[serde(alias = "promptYuanPerMillion")]
    pub prompt_yuan_per_million: Option<f64>,
    #[serde(alias = "completionYuanPerMillion")]
    pub completion_yuan_per_million: Option<f64>,
}

pub async fn set_price(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<PriceBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:write")?;
    let model = body.model.trim().to_string();
    if model.is_empty() {
        return Err(ApiError::BadRequest("缺少 model".to_string()));
    }
    let prompt = body.prompt_yuan_per_million.unwrap_or(0.0);
    let completion = body.completion_yuan_per_million.unwrap_or(0.0);
    if !prompt.is_finite() || !completion.is_finite() || prompt < 0.0 || completion < 0.0 {
        return Err(ApiError::BadRequest("单价必须是非负数".to_string()));
    }
    let price = Price {
        prompt_micro_per_million: (prompt * MICRO_PER_YUAN as f64).round() as i64,
        completion_micro_per_million: (completion * MICRO_PER_YUAN as f64).round() as i64,
    };
    billing::set_price(&state.store, &state.config.tenant_id, &model, price)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "model_price.set",
            target_type: "model",
            target_id: &model,
            before_json: None,
            after_json: Some(&format!(
                r#"{{"promptYuanPerMillion":{prompt},"completionYuanPerMillion":{completion}}}"#
            )),
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// 最近 N 条流水（给管理端看账）。
async fn recent_ledger(store: &AdminStore, user_id: &str, limit: i64) -> ApiResult<Vec<Value>> {
    let sql = store.sql(
        "SELECT kind, amount_micro, balance_after_micro, model, prompt_tokens, completion_tokens,
                note, created_at
           FROM wallet_ledger WHERE user_id = ? ORDER BY created_at DESC LIMIT ?",
    );
    let rows = sqlx::query::<sqlx::Any>(&sql)
        .bind(user_id)
        .bind(limit)
        .fetch_all(store.pool())
        .await
        .map_err(ApiError::internal)?;
    use sqlx::Row;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let amount: i64 = row.try_get(1).map_err(ApiError::internal)?;
        let after: i64 = row.try_get(2).map_err(ApiError::internal)?;
        out.push(json!({
            "kind": row.try_get::<String, _>(0).map_err(ApiError::internal)?,
            "amountYuan": billing::yuan_string(amount),
            "balanceYuan": billing::yuan_string(after),
            "model": row.try_get::<String, _>(3).map_err(ApiError::internal)?,
            "promptTokens": row.try_get::<i64, _>(4).map_err(ApiError::internal)?,
            "completionTokens": row.try_get::<i64, _>(5).map_err(ApiError::internal)?,
            "note": row.try_get::<String, _>(6).map_err(ApiError::internal)?,
            "createdAt": row.try_get::<i64, _>(7).map_err(ApiError::internal)?,
        }));
    }
    Ok(out)
}

/// 客户端侧：查自己的余额（不需要管理权限，只要登录态）。
pub async fn my_wallet(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    let balance = billing::ensure_wallet(&state.store, &state.config.tenant_id, &user.id).await?;
    let ledger = recent_ledger(&state.store, &user.id, 20).await?;
    Ok(Json(json!({
        "balanceYuan": billing::yuan_string(balance),
        "balanceMicro": balance,
        "ledger": ledger,
    })))
}
