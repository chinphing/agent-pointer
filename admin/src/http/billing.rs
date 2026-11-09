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
use crate::billing::{self, Ratio, MICRO_PER_YUAN};
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

    // 页面上按平台模型目录列出全部模型，已配置的显示现价，未配置的显示兜底价——
    // 管理员不用手打模型名，也不会看到一张空表。
    let mut configured: std::collections::BTreeMap<String, Ratio> = rows.into_iter().collect();
    let catalog: Vec<String> = crate::resources::list(
        &state.store,
        &state.config.tenant_id,
        Some(crate::resources::KIND_MODEL),
    )
    .await?
    .into_iter()
    .map(|r| r.slug)
    .collect();

    let mut names: Vec<String> = catalog;
    for name in configured.keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names.sort();
    names.dedup();

    let items: Vec<Value> = names
        .into_iter()
        .map(|model| {
            let own = configured.remove(&model);
            let configured_flag = own.is_some();
            let ratio = own.map(|r| r.as_f64()).unwrap_or(0.0);
            json!({
                "model": model,
                "configured": configured_flag,
                "ratio": ratio,
            })
        })
        .collect();
    let base = billing::base_price_micro_per_mb(&state.store).await?;
    Ok(Json(json!({
        "baseYuanPerMb": micro_per_million_to_yuan(base),
        "items": items,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PriceBody {
    pub model: String,
    /// 费率：以基准模型 1.0 为参照的倍数（官网同款）。
    pub ratio: Option<f64>,
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
    let ratio = body.ratio.unwrap_or(0.0);
    if !ratio.is_finite() || ratio < 0.0 {
        return Err(ApiError::BadRequest("费率必须是非负数".to_string()));
    }
    let price = Ratio::from_ratio(ratio);
    billing::set_price(&state.store, &state.config.tenant_id, &model, price)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "model_ratio.set",
            target_type: "model",
            target_id: &model,
            before_json: None,
            after_json: Some(&format!(r#"{{"ratio":{ratio}}}"#)),
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct BasePriceBody {
    /// 基准价：元 / MB（官网 `llm_base_yuan_per_mb`，默认 1）。
    pub base_yuan_per_mb: Option<f64>,
}

/// 改全局基准价。费率都是相对它算的，所以改它等于整体调价。
pub async fn set_base_price(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<BasePriceBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:write")?;
    let yuan = body.base_yuan_per_mb.unwrap_or(0.0);
    if !yuan.is_finite() || yuan < 0.0 {
        return Err(ApiError::BadRequest("基准价必须是非负数".to_string()));
    }
    let micro = (yuan * MICRO_PER_YUAN as f64).round() as i64;
    billing::set_base_price_micro_per_mb(&state.store, micro)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "base_price.set",
            target_type: "platform",
            target_id: "llm_base_yuan_per_mb",
            before_json: None,
            after_json: Some(&format!(r#"{{"baseYuanPerMb":{yuan}}}"#)),
        },
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// 清掉某模型的显式费率。费率清掉后模型不下发，等于不可用。
pub async fn clear_price(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(model): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:write")?;
    let model = model.trim().to_string();
    if model.is_empty() {
        return Err(ApiError::BadRequest("缺少 model".to_string()));
    }
    billing::delete_price(&state.store, &state.config.tenant_id, &model)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "model_ratio.clear",
            target_type: "model",
            target_id: &model,
            before_json: None,
            after_json: None,
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
