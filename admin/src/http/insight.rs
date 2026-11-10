//! 用量、配额、审计、总览。

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Any, Row};

use crate::audit;
use crate::db::now_ts;
use crate::http::auth::current_auth;
use crate::http::error::{ApiError, ApiResult};
use crate::oauth::token::{month_prefix, today_string};
use crate::state::Shared;
use crate::usage::{self, GroupBy};
use crate::users;

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default, alias = "groupBy")]
    pub group_by: Option<String>,
    #[serde(default, alias = "userId")]
    pub user_id: Option<String>,
}

/// `GET /api/admin/my-usage` —— 当前登录人自己的用量概览。
///
/// 与管理端 `/api/admin/usage` 的区别：`user_id` 强制取当前登录人，**不要求
/// `usage:read`**（普通成员没有任何权限，但看得见自己的账）。口径与管理端一致：
/// 数据来自 `usage_daily`，金额来自 `wallet_ledger` 的扣费流水。
pub async fn my_usage(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    let user_id = auth.user.id.clone();

    let now = now_ts();
    let from = query
        .from
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{}-01", month_prefix(now)));
    let to = query
        .to
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| today_string(now));

    let by_day = usage::aggregate_by(
        &state.store,
        &state.config.tenant_id,
        &from,
        &to,
        Some(&user_id),
        GroupBy::Day,
    )
    .await?;
    let by_model = usage::aggregate_by(
        &state.store,
        &state.config.tenant_id,
        &from,
        &to,
        Some(&user_id),
        GroupBy::Model,
    )
    .await?;

    let prompt: i64 = by_day.iter().map(|a| a.prompt).sum();
    let completion: i64 = by_day.iter().map(|a| a.completion).sum();
    let thinking: i64 = by_day.iter().map(|a| a.thinking).sum();
    let cached: i64 = by_day.iter().map(|a| a.cached).sum();

    // 金额按扣费流水合计：这才是真实扣掉的钱，和余额变化一致。
    // 区间按「日」闭区间，转成秒时右端加一天。
    let from_ts = crate::oauth::token::ts_from_day(&from).unwrap_or(0);
    let to_ts = crate::oauth::token::ts_from_day(&to)
        .map(|t| t + 86_400)
        .unwrap_or(now + 86_400);
    let (spent_micro, calls) = crate::billing::spend_in_range(&state.store, &user_id, from_ts, to_ts)
        .await
        .unwrap_or((0, 0));

    // 今日消费单独算：概览卡上「今天花了多少」比「本月」更常看。
    let today_ts = crate::oauth::token::ts_from_day(&today_string(now)).unwrap_or(now);
    let (today_micro, _) =
        crate::billing::spend_in_range(&state.store, &user_id, today_ts, today_ts + 86_400)
            .await
            .unwrap_or((0, 0));

    Ok(Json(json!({
        "from": from,
        "to": to,
        "spentYuan": crate::billing::yuan_string(spent_micro),
        "todaySpentYuan": crate::billing::yuan_string(today_micro),
        "calls": calls,
        "totals": {
            "promptTokens": prompt,
            "completionTokens": completion,
            "thinkingTokens": thinking,
            "cachedTokens": cached,
            "totalTokens": prompt + completion,
        },
        "byDay": by_day.iter().map(|a| json!({
            "day": a.key,
            "promptTokens": a.prompt,
            "completionTokens": a.completion,
            "totalTokens": a.prompt + a.completion,
        })).collect::<Vec<_>>(),
        "byModel": by_model.iter().map(|a| json!({
            "model": a.key,
            "totalTokens": a.prompt + a.completion,
        })).collect::<Vec<_>>(),
    })))
}

pub async fn usage(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:read")?;

    let now = now_ts();
    let from = query
        .from
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{}-01", month_prefix(now)));
    let to = query
        .to
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| today_string(now));

    // 总量由「按天」聚合结果累加：天数是有限的几十行，省掉第 4 次全区间扫描。
    let by_day_aggs = usage::aggregate_by(
        &state.store,
        &state.config.tenant_id,
        &from,
        &to,
        query.user_id.as_deref(),
        GroupBy::Day,
    )
    .await?;
    let prompt: i64 = by_day_aggs.iter().map(|a| a.prompt).sum();
    let completion: i64 = by_day_aggs.iter().map(|a| a.completion).sum();
    let thinking: i64 = by_day_aggs.iter().map(|a| a.thinking).sum();
    let cached: i64 = by_day_aggs.iter().map(|a| a.cached).sum();

    let to_items = |aggs: Vec<usage::UsageAgg>| -> Vec<Value> {
        aggs.into_iter()
            .map(|a| {
                json!({
                    "key": a.key,
                    "promptTokens": a.prompt,
                    "completionTokens": a.completion,
                    "thinkingTokens": a.thinking,
                    "cachedTokens": a.cached,
                    "total": a.total(),
                })
            })
            .collect()
    };

    // 成员维度用可读名展示：`key` 仍是用户 id（筛选/配额接口要它），`label` 给人看。
    // 名字缺失（用户已删）时退回 id，页面不会出现空白单元格。
    let by_user_aggs = usage::aggregate_range(
        &state.store,
        &state.config.tenant_id,
        &from,
        &to,
        query.user_id.as_deref(),
        GroupBy::User,
    )
    .await?;
    let user_ids: Vec<String> = by_user_aggs.iter().map(|a| a.key.clone()).collect();
    let users_by_id = users::display_names_by_id(&state.store, &state.config.tenant_id, &user_ids)
        .await?;

    let by_user: Vec<Value> = by_user_aggs
        .into_iter()
        .map(|a| {
            let label = users_by_id
                .get(&a.key)
                .cloned()
                .unwrap_or_else(|| a.key.clone());
            json!({
                "key": a.key,
                "label": label,
                "promptTokens": a.prompt,
                "completionTokens": a.completion,
                "thinkingTokens": a.thinking,
                "cachedTokens": a.cached,
                "total": a.total(),
            })
        })
        .collect();

    Ok(Json(json!({
        "from": from,
        "to": to,
        "totals": {
            "promptTokens": prompt,
            "completionTokens": completion,
            "thinkingTokens": thinking,
            "cachedTokens": cached,
            "total": prompt + completion,
        },
        "byUser": by_user,
        "byModel": to_items(
            usage::aggregate_range(
                &state.store,
                &state.config.tenant_id,
                &from,
                &to,
                query.user_id.as_deref(),
                GroupBy::Model,
            )
            .await?
        ),
        "byDay": to_items(by_day_aggs),
    })))
}

pub async fn get_quota(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:read")?;

    let quota = usage::get_quota(&state.store, &user_id).await?;
    let state_q = usage::quota_state(
        &state.store,
        &state.config.tenant_id,
        &user_id,
        &month_prefix(now_ts()),
    )
    .await?;

    Ok(Json(json!({
        "userId": user_id,
        "monthlyTokenLimit": quota.as_ref().and_then(|q| q.monthly_token_limit),
        "overAction": quota.map(|q| q.over_action).unwrap_or_else(|| usage::OVER_ACTION_WARN.to_string()),
        "used": state_q.used,
        "exceeded": state_q.exceeded(),
        "blocks": state_q.blocks(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct QuotaBody {
    #[serde(default, rename = "monthlyTokenLimit", alias = "monthly_token_limit")]
    pub monthly_token_limit: Option<i64>,
    #[serde(default, rename = "overAction", alias = "over_action")]
    pub over_action: Option<String>,
}

pub async fn set_quota(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(body): Json<QuotaBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("usage:write")?;

    let action = body
        .over_action
        .clone()
        .unwrap_or_else(|| usage::OVER_ACTION_WARN.to_string());
    usage::set_quota(
        &state.store,
        &state.config.tenant_id,
        &user_id,
        body.monthly_token_limit,
        &action,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        audit::AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "usage.quota",
            target_type: "user",
            target_id: &user_id,
            before_json: None,
            after_json: Some(&format!(
                r#"{{"limit":{:?},"over_action":"{}"}}"#,
                body.monthly_token_limit, action
            )),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn audit_log(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("audit:read")?;

    let entries = audit::list(
        &state.store,
        &state.config.tenant_id,
        query.limit.unwrap_or(100),
    )
    .await?;

    let items: Vec<Value> = entries
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id,
                "actorUserId": e.actor_user_id,
                "actorName": e.actor_name,
                "action": e.action,
                "targetType": e.target_type,
                "targetId": e.target_id,
                "before": e.before_json,
                "after": e.after_json,
                "createdAt": e.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "total": items.len() })))
}

pub async fn overview(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("system:read")?;

    let sql = state
        .store
        .sql("SELECT status, COUNT(*) FROM users WHERE tenant_id = ? GROUP BY status");
    let rows = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await?;
    let mut users_total = 0i64;
    let mut users_active = 0i64;
    for row in &rows {
        let status: String = row.try_get(0)?;
        let count: i64 = row.try_get(1)?;
        users_total += count;
        if status == "active" {
            users_active = count;
        }
    }

    let sql = state.store.sql(
        "SELECT kind, COUNT(*) FROM resources WHERE tenant_id = ? GROUP BY kind",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await?;
    let mut resources_by_kind = serde_json::Map::new();
    for row in &rows {
        let kind: String = row.try_get(0)?;
        let count: i64 = row.try_get(1)?;
        resources_by_kind.insert(kind, json!(count));
    }

    let today = today_string(now_ts());
    let today_agg = usage::totals(
        &state.store,
        &state.config.tenant_id,
        &today,
        &today,
        None,
    )
    .await?;
    let today_tokens: i64 = today_agg.total();

    // 近 7 天用量：按天汇总，缺的日子补 0，前端直接画柱状图。
    let mut token_trend = Vec::new();
    for back in (0..7).rev() {
        let day = today_string(now_ts() - back * 86_400);
        let sql = state.store.sql(
            "SELECT COALESCE(SUM(prompt_tokens + completion_tokens), 0) FROM usage_daily \
             WHERE tenant_id = ? AND day = ?",
        );
        let sum: i64 = sqlx::query::<Any>(&sql)
            .bind(&state.config.tenant_id)
            .bind(&day)
            .fetch_one(state.store.pool())
            .await?
            .try_get(0)?;
        token_trend.push(json!({ "day": day, "tokens": sum }));
    }

    // 最近 5 条操作：审计里已经有，不必再记一份。
    let sql = state.store.sql(
        "SELECT action, actor_name, target_type, created_at FROM audit_log \
         WHERE tenant_id = ? ORDER BY created_at DESC LIMIT 5",
    );
    let rows = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await?;
    let mut recent_actions = Vec::new();
    for row in &rows {
        recent_actions.push(json!({
            "action": row.try_get::<String, _>(0)?,
            "actorName": row.try_get::<String, _>(1)?,
            "targetType": row.try_get::<String, _>(2)?,
            "createdAt": row.try_get::<i64, _>(3)?,
        }));
    }

    Ok(Json(json!({
        "users": { "total": users_total, "active": users_active },
        "resources": resources_by_kind,
        "todayTokens": today_tokens,
        "tokenTrend": token_trend,
        "recentActions": recent_actions,
        "oauthConfigured": state.config.oauth_secret.is_some(),
    })))
}
