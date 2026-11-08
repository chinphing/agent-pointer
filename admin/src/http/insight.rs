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

    let rows = usage::list(
        &state.store,
        &state.config.tenant_id,
        &from,
        &to,
        query.user_id.as_deref(),
    )
    .await?;

    let prompt: i64 = rows.iter().map(|r| r.prompt_tokens).sum();
    let completion: i64 = rows.iter().map(|r| r.completion_tokens).sum();

    let to_items = |pairs: Vec<(String, i64, i64)>| -> Vec<Value> {
        pairs
            .into_iter()
            .map(|(key, p, c)| json!({ "key": key, "promptTokens": p, "completionTokens": c, "total": p + c }))
            .collect()
    };

    Ok(Json(json!({
        "from": from,
        "to": to,
        "totals": { "promptTokens": prompt, "completionTokens": completion, "total": prompt + completion },
        "byUser": to_items(usage::aggregate(&rows, GroupBy::User)),
        "byModel": to_items(usage::aggregate(&rows, GroupBy::Model)),
        "byDay": to_items(usage::aggregate(&rows, GroupBy::Day)),
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
    let today_rows = usage::list(
        &state.store,
        &state.config.tenant_id,
        &today,
        &today,
        None,
    )
    .await?;
    let today_tokens: i64 = today_rows.iter().map(|r| r.total()).sum();

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
