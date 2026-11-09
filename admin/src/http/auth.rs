//! 管理端网页会话：Cookie + RBAC 校验。

use std::collections::HashSet;

use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::error::{ApiError, ApiResult};
use crate::rbac;
use crate::sessions;
use crate::state::Shared;
use crate::users::{self, User};

#[derive(Debug)]
pub struct AdminAuth {
    pub user: User,
    pub permissions: HashSet<String>,
    pub roles: Vec<String>,
}

impl AdminAuth {
    pub fn has(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    pub fn require(&self, permission: &str) -> ApiResult<()> {
        if self.has(permission) {
            Ok(())
        } else {
            Err(ApiError::Forbidden(format!("缺少权限：{permission}")))
        }
    }

    pub fn is_super_admin(&self) -> bool {
        self.roles.iter().any(|r| r == rbac::SUPER_ADMIN)
    }
}

/// 从 `Cookie` 头里取指定名字的值（不引 cookie crate，格式足够简单）。
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

pub async fn current_auth(state: &Shared, headers: &HeaderMap) -> ApiResult<AdminAuth> {
    let token = cookie_value(headers, &state.config.cookie_name).ok_or(ApiError::Unauthorized)?;
    let session = sessions::lookup(&state.store, &token)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if session.kind != sessions::KIND_ADMIN {
        return Err(ApiError::Unauthorized);
    }
    let user = users::get(&state.store, &session.user_id)
        .await?
        .filter(|u| u.is_active())
        .ok_or(ApiError::Unauthorized)?;

    let permissions = rbac::permissions_for_user(&state.store, &user.id).await?;
    let roles = rbac::role_keys_for_user(&state.store, &user.id).await?;
    Ok(AdminAuth {
        user,
        permissions,
        roles,
    })
}

fn session_cookie(state: &Shared, token: &str) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        state.config.cookie_name, token, state.config.session_ttl_secs
    );
    if state.config.cookie_secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn cleared_cookie(state: &Shared) -> String {
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        state.config.cookie_name
    )
}

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    pub username: String,
    pub password: String,
    #[serde(default, rename = "captchaId", alias = "captcha_id")]
    pub captcha_id: String,
    #[serde(default)]
    pub captcha: String,
}

/// 发一张登录验证码。公开接口——登录前就要能拿到。
///
/// 同时告诉前端「这次登录要不要验证码」：关掉验证码时前端不该显示这个框，
/// 否则必填项会把登录卡住。
pub async fn issue_captcha(State(state): State<Shared>) -> ApiResult<Json<Value>> {
    if !state.config.captcha_required {
        return Ok(Json(json!({ "required": false })));
    }
    let (id, image) = crate::captcha::issue(&state.store)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "required": true, "id": id, "image": image })))
}

pub async fn login(
    State(state): State<Shared>,
    Json(body): Json<LoginBody>,
) -> ApiResult<Response> {
    // 先验验证码再验密码：撞库脚本连密码都试不到。
    if state.config.captcha_required {
        let captcha_id = body.captcha_id.trim();
        let captcha_answer = body.captcha.trim();
        if captcha_id.is_empty() || captcha_answer.is_empty() {
            return Err(ApiError::BadRequest("请填写验证码".into()));
        }
        if !crate::captcha::consume(&state.store, captcha_id, captcha_answer)
            .await
            .map_err(ApiError::internal)?
        {
            return Err(ApiError::BadRequest("验证码不正确或已过期，请刷新重试".into()));
        }
    }

    let user = users::authenticate(
        &state.store,
        &state.config.tenant_id,
        &body.username,
        &body.password,
    )
    .await?;

    let Some(user) = user else {
        crate::audit::record(
            &state.store,
            &state.config.tenant_id,
            crate::audit::AuditInput {
                actor_user_id: None,
                actor_name: body.username.trim(),
                action: "login_failed",
                target_type: "user",
                target_id: "",
                before_json: None,
                after_json: None,
            },
        )
        .await?;
        return Err(ApiError::InvalidCredentials);
    };

    let token = sessions::create(
        &state.store,
        &user.id,
        sessions::KIND_ADMIN,
        state.config.session_ttl_secs,
    )
    .await?;
    let permissions = rbac::permissions_for_user(&state.store, &user.id).await?;
    let roles = rbac::role_keys_for_user(&state.store, &user.id).await?;

    crate::audit::record(
        &state.store,
        &state.config.tenant_id,
        crate::audit::AuditInput {
            actor_user_id: Some(&user.id),
            actor_name: &user.display_name,
            action: "login",
            target_type: "user",
            target_id: &user.id,
            before_json: None,
            after_json: None,
        },
    )
    .await?;

    let body = json!({
        "user": {
            "id": user.id,
            "username": user.username,
            "displayName": user.display_name,
        },
        "roles": roles,
        "permissions": permissions.into_iter().collect::<Vec<_>>(),
    });

    Ok((
        [(header::SET_COOKIE, session_cookie(&state, &token))],
        Json(body),
    )
        .into_response())
}

pub async fn logout(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = cookie_value(&headers, &state.config.cookie_name) {
        sessions::revoke(&state.store, &token).await?;
    }
    Ok((
        [(header::SET_COOKIE, cleared_cookie(&state))],
        Json(json!({ "ok": true })),
    )
        .into_response())
}

pub async fn me(State(state): State<Shared>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    Ok(Json(json!({
        "user": {
            "id": auth.user.id,
            "username": auth.user.username,
            "displayName": auth.user.display_name,
            "status": auth.user.status,
            "lastLoginAt": auth.user.last_login_at,
        },
        "roles": auth.roles,
        "permissions": auth.permissions.into_iter().collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AdminConfig;
    use crate::db::AdminStore;
    use crate::schema::DEFAULT_TENANT;
    use axum::http::HeaderValue;
    use std::sync::Arc;

    async fn state() -> Shared {
        let store = AdminStore::connect_in_memory().await.expect("store");
        Arc::new(crate::state::AppState {
            store,
            config: AdminConfig::default(),
        })
    }

    #[test]
    fn cookie_parsing_picks_named_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; pointer_admin_session=abc; b=2"),
        );
        assert_eq!(
            cookie_value(&headers, "pointer_admin_session").as_deref(),
            Some("abc")
        );
        assert!(cookie_value(&headers, "missing").is_none());
    }

    #[tokio::test]
    async fn no_cookie_means_unauthorized() {
        let state = state().await;
        assert!(matches!(
            current_auth(&state, &HeaderMap::new()).await.unwrap_err(),
            ApiError::Unauthorized
        ));
    }

    #[tokio::test]
    async fn admin_session_resolves_permissions() {
        let state = state().await;
        let user = users::create(&state.store, DEFAULT_TENANT, "root", "管理员", "pw-123456")
            .await
            .expect("user");
        rbac::assign_role(&state.store, DEFAULT_TENANT, &user.id, rbac::SUPER_ADMIN)
            .await
            .expect("role");
        let token = sessions::create(&state.store, &user.id, sessions::KIND_ADMIN, 3600)
            .await
            .expect("session");

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("pointer_admin_session={token}")).expect("cookie"),
        );
        let auth = current_auth(&state, &headers).await.expect("auth");
        assert!(auth.is_super_admin());
        assert!(auth.has("user:write"));
    }

    #[tokio::test]
    async fn access_token_is_not_a_web_session() {
        let state = state().await;
        let user = users::create(&state.store, DEFAULT_TENANT, "u", "U", "pw-123456")
            .await
            .expect("user");
        let token = sessions::create(&state.store, &user.id, sessions::KIND_ACCESS, 3600)
            .await
            .expect("session");

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("pointer_admin_session={token}")).expect("cookie"),
        );
        assert!(matches!(
            current_auth(&state, &headers).await.unwrap_err(),
            ApiError::Unauthorized
        ));
    }
}
