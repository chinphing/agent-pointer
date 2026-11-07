//! `POST /auth/oauth/exchange-code`、`POST /auth/app/token`、`POST /auth/app/logout`。
//!
//! 两个 token 接口返回的字段集合一样，但 `user` 的命名不同：
//! `/auth/app/token` 是 snake_case（`is_platform_admin`），
//! `/auth/oauth/exchange-code` 是 camelCase（`isPlatformAdmin`）。
//! 这不是笔误——两处客户端的解析结构体就是这么写的，照抄才不会被踢下线。

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::credentials;
use crate::db::now_ts;
use crate::http::error::{ApiError, ApiResult};
use crate::oauth::clients::{self, CLIENT_SERVER};
use crate::oauth::code::verify_pkce;
use crate::sessions;
use crate::state::Shared;
use crate::usage;
use crate::users::{self, User};

/// 把 unix 秒拆成 (年, 月, 日)。不引 chrono，避免为一个日期加一个大依赖。
fn civil_from_ts(ts: i64) -> (i64, u32, u32) {
    let days = ts.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

pub fn today_string(ts: i64) -> String {
    let (y, m, d) = civil_from_ts(ts);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn month_prefix(ts: i64) -> String {
    let (y, m, _) = civil_from_ts(ts);
    format!("{y:04}-{m:02}")
}

#[derive(Debug, Deserialize)]
pub struct ExchangeBody {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    pub code: String,
    pub code_verifier: String,
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppTokenBody {
    #[serde(default)]
    pub grant_type: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub code_verifier: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

fn require_configured(state: &Shared) -> ApiResult<()> {
    if state.config.oauth_secret.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "控制面尚未配置 OAuth 客户端（POINTER_ADMIN_OAUTH_SECRET）".to_string(),
        ));
    }
    Ok(())
}

async fn check_client(
    state: &Shared,
    client_id: &str,
    client_secret: Option<&str>,
) -> ApiResult<()> {
    // 桌面端是公共客户端：没有 secret，安全性由 PKCE 承担。
    if clients::is_public_client(&state.store, client_id).await? {
        return Ok(());
    }
    let Some(secret) = client_secret.filter(|s| !s.trim().is_empty()) else {
        return Err(ApiError::Unauthorized);
    };
    if !clients::verify_secret(&state.store, client_id, secret).await? {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

/// 用户配额快照：(included, consumed, exhausted)。
async fn quota_snapshot(state: &Shared, user_id: &str) -> ApiResult<(u64, u64, bool)> {
    let state_q = usage::quota_state(
        &state.store,
        &state.config.tenant_id,
        user_id,
        &month_prefix(now_ts()),
    )
    .await?;
    Ok((
        state_q.limit.unwrap_or(0).max(0) as u64,
        state_q.used.max(0) as u64,
        state_q.blocks(),
    ))
}

fn user_json(user: &User, quota: (u64, u64, bool), snake: bool) -> Value {
    let (included, consumed, exhausted) = quota;
    if snake {
        json!({
            "id": user.id,
            "nickname": user.display_name,
            "is_platform_admin": false,
            "included_tokens": included,
            "consumed_tokens": consumed,
            "token_quota_exhausted": exhausted,
        })
    } else {
        json!({
            "id": user.id,
            "nickname": user.display_name,
            "isPlatformAdmin": false,
            "includedTokens": included,
            "consumedTokens": consumed,
            "tokenQuotaExhausted": exhausted,
        })
    }
}

async fn build_token_payload(
    state: &Shared,
    user: &User,
    access_token: &str,
    refresh_token: Option<&str>,
    snake_user: bool,
) -> ApiResult<Value> {
    let creds =
        credentials::build_for_user(&state.store, &state.config.tenant_id, &user.id).await?;
    let quota = quota_snapshot(state, &user.id).await?;

    let mut body = serde_json::to_value(&creds).map_err(ApiError::internal)?;
    let map = body
        .as_object_mut()
        .ok_or_else(|| ApiError::Internal("credentials 不是对象".to_string()))?;
    map.insert("access_token".to_string(), json!(access_token));
    map.insert(
        "expires_in".to_string(),
        json!(state.config.access_token_ttl_secs.max(60)),
    );
    map.insert("agent_id".to_string(), json!(""));
    map.insert("user".to_string(), user_json(user, quota, snake_user));
    if let Some(refresh) = refresh_token {
        map.insert("refresh_token".to_string(), json!(refresh));
    }
    Ok(body)
}

/// `/auth/oauth/exchange-code` 的核心逻辑（handler 只是薄壳，便于单测）。
pub async fn do_exchange_code(state: &Shared, body: ExchangeBody) -> ApiResult<Value> {
    require_configured(state)?;
    let client_id = body
        .client_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(CLIENT_SERVER);
    check_client(state, client_id, body.client_secret.as_deref()).await?;

    let pending = crate::oauth::code::consume(&state.store, &body.code)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::BadRequest("授权码无效或已过期".to_string()))?;
    if pending.client_id != client_id {
        return Err(ApiError::BadRequest("授权码与客户端不匹配".to_string()));
    }
    if let Some(uri) = body.redirect_uri.as_deref() {
        if !uri.is_empty() && uri != pending.redirect_uri {
            return Err(ApiError::BadRequest("redirect_uri 与授权时不一致".to_string()));
        }
    }
    if !verify_pkce(&body.code_verifier, &pending.code_challenge) {
        return Err(ApiError::BadRequest("code_verifier 校验失败".to_string()));
    }

    let user = users::get(&state.store, &pending.user_id)
        .await?
        .filter(|u| u.is_active())
        .ok_or(ApiError::Unauthorized)?;

    let access = sessions::create(
        &state.store,
        &user.id,
        sessions::KIND_ACCESS,
        state.config.access_token_ttl_secs,
    )
    .await?;
    let refresh = sessions::create(
        &state.store,
        &user.id,
        sessions::KIND_REFRESH,
        state.config.refresh_token_ttl_secs,
    )
    .await?;

    build_token_payload(state, &user, &access, Some(&refresh), false).await
}

pub async fn do_app_token(state: &Shared, body: AppTokenBody) -> ApiResult<Value> {
    require_configured(state)?;
    let client_id = body
        .client_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(CLIENT_SERVER);

    let is_refresh = body
        .grant_type
        .as_deref()
        .map(|g| g == "refresh_token")
        .unwrap_or(false);

    if is_refresh {
        // 刷新不需要 client_secret：refresh token 本身就是凭据（轮换时再收紧）。
        let Some(refresh) = body.refresh_token.as_deref().filter(|s| !s.is_empty()) else {
            return Err(ApiError::BadRequest("缺少 refresh_token".to_string()));
        };
        let session = sessions::lookup(&state.store, refresh)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        if session.kind != sessions::KIND_REFRESH {
            return Err(ApiError::Unauthorized);
        }
        let user = users::get(&state.store, &session.user_id)
            .await?
            .filter(|u| u.is_active())
            .ok_or(ApiError::Unauthorized)?;
        let access = sessions::create(
            &state.store,
            &user.id,
            sessions::KIND_ACCESS,
            state.config.access_token_ttl_secs,
        )
        .await?;
        return build_token_payload(state, &user, &access, Some(refresh), true).await;
    }

    check_client(state, client_id, body.client_secret.as_deref()).await?;
    let code = body
        .code
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::BadRequest("缺少 code".to_string()))?;
    let verifier = body.code_verifier.as_deref().unwrap_or_default();

    let pending = crate::oauth::code::consume(&state.store, code)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::BadRequest("授权码无效或已过期".to_string()))?;
    if pending.client_id != client_id {
        return Err(ApiError::BadRequest("授权码与客户端不匹配".to_string()));
    }
    if !verify_pkce(verifier, &pending.code_challenge) {
        return Err(ApiError::BadRequest("code_verifier 校验失败".to_string()));
    }

    let user = users::get(&state.store, &pending.user_id)
        .await?
        .filter(|u| u.is_active())
        .ok_or(ApiError::Unauthorized)?;
    let access = sessions::create(
        &state.store,
        &user.id,
        sessions::KIND_ACCESS,
        state.config.access_token_ttl_secs,
    )
    .await?;
    let refresh = sessions::create(
        &state.store,
        &user.id,
        sessions::KIND_REFRESH,
        state.config.refresh_token_ttl_secs,
    )
    .await?;
    build_token_payload(state, &user, &access, Some(&refresh), true).await
}

pub async fn exchange_code(
    State(state): State<Shared>,
    Json(body): Json<ExchangeBody>,
) -> ApiResult<Json<Value>> {
    Ok(Json(do_exchange_code(&state, body).await?))
}

pub async fn app_token(
    State(state): State<Shared>,
    Json(body): Json<AppTokenBody>,
) -> ApiResult<Json<Value>> {
    Ok(Json(do_app_token(&state, body).await?))
}

#[derive(Debug, Deserialize)]
pub struct LogoutBody {
    #[serde(default)]
    pub refresh_token: Option<String>,
}

pub async fn app_logout(
    State(state): State<Shared>,
    _headers: HeaderMap,
    Json(body): Json<LogoutBody>,
) -> ApiResult<Json<Value>> {
    if let Some(token) = body.refresh_token.as_deref().filter(|s| !s.is_empty()) {
        sessions::revoke(&state.store, token).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encrypt_secret;
    use crate::grants::{self, Principal};
    use crate::resources::{self, KIND_MODEL, KIND_PROVIDER};
    use crate::schema::DEFAULT_TENANT;
    use crate::{config::AdminConfig, db::AdminStore};
    use std::sync::Arc;

    const SECRET: &str = "test-oauth-secret";

    async fn state() -> Shared {
        let store = AdminStore::connect_in_memory().await.expect("store");
        clients::upsert(&store, CLIENT_SERVER, SECRET, clients::KIND_SERVER, &[])
            .await
            .expect("client");
        let cfg = AdminConfig {
            oauth_secret: Some(SECRET.to_string()),
            ..Default::default()
        };
        Arc::new(crate::state::AppState {
            store,
            config: cfg,
        })
    }

    async fn user_with_provider(state: &Shared) -> String {
        let uid = users::create(&state.store, DEFAULT_TENANT, "alice", "爱丽丝", "pw-123456")
            .await
            .expect("user")
            .id;
        let enc = encrypt_secret("sk-live").expect("encrypt");
        let provider = resources::create(
            &state.store,
            DEFAULT_TENANT,
            KIND_PROVIDER,
            "qwen",
            "阿里云百炼",
            "",
            None,
            r#"{"baseUrl":"https://example.invalid/v1"}"#,
            Some(&enc),
        )
        .await
        .expect("provider");
        resources::set_enabled(&state.store, &provider.id, true)
            .await
            .expect("enable");
        grants::replace_grants(&state.store, &provider.id, &[Principal::All])
            .await
            .expect("grant provider");
        let model = resources::create(
            &state.store,
            DEFAULT_TENANT,
            KIND_MODEL,
            "m1",
            "m1",
            "",
            Some(&provider.id),
            "{}",
            None,
        )
        .await
        .expect("model");
        resources::set_enabled(&state.store, &model.id, true)
            .await
            .expect("enable model");
        grants::replace_grants(&state.store, &model.id, &[Principal::All])
            .await
            .expect("grant model");
        uid
    }

    async fn issue_code(state: &Shared, uid: &str, verifier: &str) -> String {
        let challenge = {
            use base64::Engine;
            use sha2::{Digest, Sha256};
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(Sha256::digest(verifier.as_bytes()))
        };
        crate::oauth::code::issue(
            &state.store,
            CLIENT_SERVER,
            uid,
            "http://127.0.0.1:8787/auth/callback",
            &challenge,
        )
        .await
        .expect("issue code")
    }

    #[tokio::test]
    async fn exchange_happy_path_returns_credentials() {
        let state = state().await;
        let uid = user_with_provider(&state).await;
        let code = issue_code(&state, &uid, "verifier-123").await;

        let body = do_exchange_code(
            &state,
            ExchangeBody {
                client_id: Some(CLIENT_SERVER.to_string()),
                client_secret: Some(SECRET.to_string()),
                code,
                code_verifier: "verifier-123".to_string(),
                redirect_uri: None,
            },
        )
        .await
        .expect("exchange");

        assert!(body["access_token"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(body["refresh_token"].as_str().is_some());
        assert_eq!(body["provider_api_keys"]["qwen"], "sk-live");
        assert_eq!(body["modelCatalog"]["qwen"][0], "m1");
        assert!(body["user"]["isPlatformAdmin"].is_boolean(), "exchange 用 camelCase");
    }

    #[tokio::test]
    async fn wrong_client_secret_is_unauthorized() {
        let state = state().await;
        let uid = user_with_provider(&state).await;
        let code = issue_code(&state, &uid, "v").await;
        let err = do_exchange_code(
            &state,
            ExchangeBody {
                client_id: Some(CLIENT_SERVER.to_string()),
                client_secret: Some("wrong".to_string()),
                code,
                code_verifier: "v".to_string(),
                redirect_uri: None,
            },
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, ApiError::Unauthorized));
    }

    #[tokio::test]
    async fn bad_pkce_is_rejected_and_code_is_burned() {
        let state = state().await;
        let uid = user_with_provider(&state).await;
        let code = issue_code(&state, &uid, "right").await;

        let err = do_exchange_code(
            &state,
            ExchangeBody {
                client_id: Some(CLIENT_SERVER.to_string()),
                client_secret: Some(SECRET.to_string()),
                code: code.clone(),
                code_verifier: "wrong".to_string(),
                redirect_uri: None,
            },
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, ApiError::BadRequest(_)));

        // 授权码已被消费，重试也换不到
        let err = do_exchange_code(
            &state,
            ExchangeBody {
                client_id: Some(CLIENT_SERVER.to_string()),
                client_secret: Some(SECRET.to_string()),
                code,
                code_verifier: "right".to_string(),
                redirect_uri: None,
            },
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, ApiError::BadRequest(_)));
    }

    #[tokio::test]
    async fn app_token_uses_snake_case_user_and_supports_refresh() {
        let state = state().await;
        let uid = user_with_provider(&state).await;
        let code = issue_code(&state, &uid, "v2").await;

        let first = do_app_token(
            &state,
            AppTokenBody {
                grant_type: None,
                code: Some(code),
                code_verifier: Some("v2".to_string()),
                refresh_token: None,
                client_id: Some(CLIENT_SERVER.to_string()),
                client_secret: Some(SECRET.to_string()),
                redirect_uri: None,
            },
        )
        .await
        .expect("token");
        assert!(first["user"]["is_platform_admin"].is_boolean(), "app/token 用 snake_case");
        let refresh = first["refresh_token"].as_str().expect("refresh").to_string();

        let second = do_app_token(
            &state,
            AppTokenBody {
                grant_type: Some("refresh_token".to_string()),
                code: None,
                code_verifier: None,
                refresh_token: Some(refresh.clone()),
                client_id: None,
                client_secret: None,
                redirect_uri: None,
            },
        )
        .await
        .expect("refresh");
        assert_ne!(first["access_token"], second["access_token"]);
        assert_eq!(second["refresh_token"], refresh, "刷新不轮换 refresh token");
    }

    #[tokio::test]
    async fn unconfigured_oauth_returns_503() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let state = Arc::new(crate::state::AppState {
            store,
            config: AdminConfig::default(),
        });
        let err = do_exchange_code(
            &state,
            ExchangeBody {
                client_id: None,
                client_secret: None,
                code: "x".into(),
                code_verifier: "y".into(),
                redirect_uri: None,
            },
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, ApiError::ServiceUnavailable(_)));
    }

    #[test]
    fn civil_date_conversion() {
        assert_eq!(today_string(0), "1970-01-01");
        assert_eq!(month_prefix(0), "1970-01");
        // 2026-11-07 00:00:00 UTC
        assert_eq!(today_string(1_794_019_200), "2026-11-07");
        assert_eq!(month_prefix(1_794_019_200), "2026-11");
    }
}
