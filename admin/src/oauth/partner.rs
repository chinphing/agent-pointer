//! 面向 server / 客户端的 partner 接口（Bearer access token）。

use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::credentials;
use crate::db::now_ts;
use crate::http::error::{ApiError, ApiResult};
use crate::oauth::token::{month_prefix, today_string};
use crate::sessions;
use crate::state::Shared;
use crate::usage;
use crate::users::{self, User};

pub async fn bearer_user(state: &Shared, headers: &HeaderMap) -> ApiResult<User> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(ApiError::Unauthorized)?;

    let session = sessions::lookup(&state.store, token)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if session.kind != sessions::KIND_ACCESS {
        return Err(ApiError::Unauthorized);
    }
    users::get(&state.store, &session.user_id)
        .await?
        .filter(|u| u.is_active())
        .ok_or(ApiError::Unauthorized)
}

async fn quota_json(state: &Shared, user_id: &str) -> ApiResult<Value> {
    let q = usage::quota_state(
        &state.store,
        &state.config.tenant_id,
        user_id,
        &month_prefix(now_ts()),
    )
    .await?;
    Ok(json!({
        "limit": q.limit,
        "used": q.used,
        "exceeded": q.exceeded(),
        "over_action": q.over_action,
    }))
}

pub async fn llm_credentials(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    let creds =
        credentials::build_for_user(&state.store, &state.config.tenant_id, &user.id).await?;
    Ok(Json(serde_json::to_value(&creds).map_err(ApiError::internal)?))
}

pub async fn balance(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    let q = usage::quota_state(
        &state.store,
        &state.config.tenant_id,
        &user.id,
        &month_prefix(now_ts()),
    )
    .await?;
    Ok(Json(json!({
        // 控制面不接计费系统，余额恒为 0.00；客户端只是展示，不做扣减判断。
        "balance_yuan": "0.00",
        "token_quota_exhausted": q.blocks(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct UsageReport {
    #[serde(default)]
    pub model: String,
    #[serde(default, alias = "promptTokens")]
    pub prompt_tokens: i64,
    #[serde(default, alias = "completionTokens")]
    pub completion_tokens: i64,
    #[serde(default)]
    pub day: Option<String>,
}

pub async fn token_usage(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<UsageReport>,
) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    let day = body
        .day
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| today_string(now_ts()));

    usage::record(
        &state.store,
        &state.config.tenant_id,
        &user.id,
        body.model.trim(),
        &day,
        body.prompt_tokens,
        body.completion_tokens,
    )
    .await?;

    let q = usage::quota_state(
        &state.store,
        &state.config.tenant_id,
        &user.id,
        &month_prefix(now_ts()),
    )
    .await?;

    Ok(Json(json!({
        "id": user.id,
        "received": true,
        "balance_yuan": "0.00",
        "token_quota_exhausted": q.blocks(),
    })))
}

/// `GET /auth/me`：给客户端展示「我是谁、还剩多少额度」。
pub async fn me(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    let quota = quota_json(&state, &user.id).await?;
    Ok(Json(json!({
        "id": user.id,
        "username": user.username,
        "displayName": user.display_name,
        "quota": quota,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AdminConfig;
    use crate::db::AdminStore;
    use crate::oauth::code::CodeStore;
    use crate::schema::DEFAULT_TENANT;
    use axum::http::HeaderValue;
    use std::sync::Arc;

    async fn state_with_user() -> (Shared, String) {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = users::create(&store, DEFAULT_TENANT, "bob", "鲍勃", "pw-123456")
            .await
            .expect("user")
            .id;
        let shared = Arc::new(crate::state::AppState {
            store,
            config: AdminConfig::default(),
            codes: CodeStore::default(),
        });
        (shared, uid)
    }

    fn bearer(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).expect("header"),
        );
        headers
    }

    #[tokio::test]
    async fn missing_or_bad_token_is_unauthorized() {
        let (state, _uid) = state_with_user().await;
        assert!(matches!(
            bearer_user(&state, &HeaderMap::new()).await.unwrap_err(),
            ApiError::Unauthorized
        ));
        assert!(matches!(
            bearer_user(&state, &bearer("nope")).await.unwrap_err(),
            ApiError::Unauthorized
        ));
    }

    #[tokio::test]
    async fn refresh_token_cannot_be_used_as_access_token() {
        let (state, uid) = state_with_user().await;
        let refresh = sessions::create(&state.store, &uid, sessions::KIND_REFRESH, 3600)
            .await
            .expect("refresh");
        assert!(matches!(
            bearer_user(&state, &bearer(&refresh)).await.unwrap_err(),
            ApiError::Unauthorized
        ));
    }

    #[tokio::test]
    async fn access_token_resolves_user() {
        let (state, uid) = state_with_user().await;
        let access = sessions::create(&state.store, &uid, sessions::KIND_ACCESS, 3600)
            .await
            .expect("access");
        let user = bearer_user(&state, &bearer(&access)).await.expect("user");
        assert_eq!(user.id, uid);
    }

    #[tokio::test]
    async fn usage_report_accumulates_tokens() {
        let (state, uid) = state_with_user().await;
        let access = sessions::create(&state.store, &uid, sessions::KIND_ACCESS, 3600)
            .await
            .expect("access");

        let _ = token_usage(
            State(state.clone()),
            bearer(&access),
            Json(UsageReport {
                model: "qwen".into(),
                prompt_tokens: 10,
                completion_tokens: 5,
                day: Some("2026-11-07".into()),
            }),
        )
        .await
        .expect("report");

        let rows = usage::list(
            &state.store,
            DEFAULT_TENANT,
            "2026-11-01",
            "2026-11-30",
            Some(&uid),
        )
        .await
        .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total(), 15);
    }
}
