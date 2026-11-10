//! 员工账号管理。

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Any, Row};

use crate::audit::{self, AuditInput};
use crate::http::auth::current_auth;
use crate::http::error::{ApiError, ApiResult};
use crate::rbac;
use crate::sessions;
use crate::state::Shared;
use crate::users::{self, STATUS_ACTIVE, STATUS_DISABLED};

async fn role_map(state: &Shared) -> ApiResult<HashMap<String, Vec<String>>> {
    let sql = state.store.sql(
        "SELECT ur.user_id, r.role_key FROM user_roles ur
         JOIN roles r ON r.id = ur.role_id ORDER BY r.role_key",
    );
    let rows = sqlx::query::<Any>(&sql).fetch_all(state.store.pool()).await?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for row in &rows {
        let user_id: String = row.try_get(0)?;
        let key: String = row.try_get(1)?;
        map.entry(user_id).or_default().push(key);
    }
    Ok(map)
}

/// 所有用户的钱包余额（微元）。一次查全表，避免 N+1。
async fn wallet_map(state: &Shared) -> Result<HashMap<String, i64>, ApiError> {
    let sql = state
        .store
        .sql("SELECT user_id, balance_micro FROM wallets WHERE tenant_id = ?");
    let rows = sqlx::query::<Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_all(state.store.pool())
        .await
        .map_err(ApiError::internal)?;
    let mut map = HashMap::new();
    for row in rows {
        let user_id: String = row.try_get(0).map_err(ApiError::internal)?;
        let balance: i64 = row.try_get(1).map_err(ApiError::internal)?;
        map.insert(user_id, balance);
    }
    Ok(map)
}

pub async fn list_users(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:read")?;

    let users = users::list(&state.store, &state.config.tenant_id).await?;
    let roles = role_map(&state).await?;
    let wallets = wallet_map(&state).await?;
    let items: Vec<Value> = users
        .iter()
        .map(|u| {
            json!({
                "id": u.id,
                "username": u.username,
                "displayName": u.display_name,
                "phone": u.phone,
                "status": u.status,
                "roles": roles.get(&u.id).cloned().unwrap_or_default(),
                "createdAt": u.created_at,
                "lastLoginAt": u.last_login_at,
                // 还没用过的人没有钱包行，给 null 让前端区分「没开」和「0 元」。
                "balanceYuan": wallets.get(&u.id).map(|v| crate::billing::yuan_string(*v)),
            })
        })
        .collect();
    Ok(Json(json!({ "items": items, "total": items.len() })))
}

#[derive(Debug, Deserialize)]
pub struct CreateUserBody {
    pub username: String,
    #[serde(default, rename = "displayName", alias = "display_name")]
    pub display_name: String,
    pub password: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

async fn apply_roles(state: &Shared, user_id: &str, roles: &[String]) -> ApiResult<()> {
    for key in roles {
        if !rbac::is_builtin(key) {
            return Err(ApiError::BadRequest(format!("未知角色：{key}")));
        }
    }
    let refs: Vec<&str> = roles.iter().map(String::as_str).collect();
    rbac::replace_roles(&state.store, &state.config.tenant_id, user_id, &refs).await?;
    Ok(())
}

pub async fn create_user(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<CreateUserBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:write")?;

    let user = users::create(
        &state.store,
        &state.config.tenant_id,
        &body.username,
        &body.display_name,
        &body.password,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    apply_roles(&state, &user.id, &body.roles).await?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "user.create",
            target_type: "user",
            target_id: &user.id,
            before_json: None,
            after_json: Some(&format!(
                r#"{{"username":"{}","roles":{:?}}}"#,
                user.username, body.roles
            )),
        },
    )
    .await?;

    Ok(Json(json!({
        "id": user.id,
        "username": user.username,
        "displayName": user.display_name,
        "phone": user.phone,
        "status": user.status,
        "roles": body.roles,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PhoneBody {
    #[serde(default)]
    pub phone: String,
}

/// 维护成员手机号（开源仓只存不用；私有仓的短信登录复用这一列）。
pub async fn set_user_phone(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(body): Json<PhoneBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:write")?;

    let phone = body.phone.trim();
    users::set_phone(
        &state.store,
        &state.config.tenant_id,
        &user_id,
        if phone.is_empty() { None } else { Some(phone) },
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "user.phone",
            target_type: "user",
            target_id: &user_id,
            before_json: None,
            after_json: Some(&format!(r#"{{"phone":"{phone}"}}"#)),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true, "phone": phone })))
}

#[derive(Debug, Deserialize)]
pub struct StatusBody {
    pub status: String,
}

pub async fn set_user_status(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(body): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:write")?;
    if body.status != STATUS_ACTIVE && body.status != STATUS_DISABLED {
        return Err(ApiError::BadRequest(format!(
            "未知状态：{}",
            body.status
        )));
    }
    if body.status == STATUS_DISABLED && user_id == auth.user.id {
        return Err(ApiError::BadRequest("不能禁用自己".to_string()));
    }

    users::set_status(&state.store, &user_id, &body.status)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if body.status == STATUS_DISABLED {
        sessions::revoke_all_for_user(&state.store, &user_id).await?;
    }

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "user.status",
            target_type: "user",
            target_id: &user_id,
            before_json: None,
            after_json: Some(&format!(r#"{{"status":"{}"}}"#, body.status)),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct PasswordBody {
    pub password: String,
}

pub async fn reset_user_password(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(body): Json<PasswordBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:write")?;

    users::reset_password(&state.store, &user_id, &body.password)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    sessions::revoke_all_for_user(&state.store, &user_id).await?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "user.reset_password",
            target_type: "user",
            target_id: &user_id,
            before_json: None,
            after_json: None,
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct RolesBody {
    #[serde(default)]
    pub roles: Vec<String>,
}

pub async fn set_user_roles(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(body): Json<RolesBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:write")?;
    if user_id == auth.user.id && !body.roles.iter().any(|r| r == rbac::SUPER_ADMIN) {
        // 允许自己降权会导致「最后一个管理员把自己锁在门外」。
        return Err(ApiError::BadRequest(
            "不能移除自己的 super_admin 角色".to_string(),
        ));
    }
    apply_roles(&state, &user_id, &body.roles).await?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "user.roles",
            target_type: "user",
            target_id: &user_id,
            before_json: None,
            after_json: Some(&format!(r#"{{"roles":{:?}}}"#, body.roles)),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true, "roles": body.roles })))
}

#[derive(Debug, Deserialize)]
pub struct ImportBody {
    /// CSV 文本：`username,display_name,password[,role]`，首行可为表头。
    pub csv: String,
    #[serde(default, alias = "defaultRole")]
    pub default_role: Option<String>,
}

/// 批量导入。逐行处理，坏行不影响好行，最后统一报告。
pub async fn import_users(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<ImportBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require("user:write")?;

    let default_role = body
        .default_role
        .clone()
        .unwrap_or_else(|| rbac::ROLE_MEMBER.to_string());

    let mut created = 0usize;
    let mut skipped: Vec<Value> = Vec::new();

    for (index, line) in body.csv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if index == 0 && line.to_ascii_lowercase().starts_with("username") {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < 3 {
            skipped.push(json!({ "line": index + 1, "reason": "列数不足（需要 账号,姓名,初始密码）" }));
            continue;
        }
        let roles: Vec<String> = if cols.len() >= 4 && !cols[3].is_empty() {
            vec![cols[3].to_string()]
        } else {
            vec![default_role.clone()]
        };
        match users::create(&state.store, &state.config.tenant_id, cols[0], cols[1], cols[2]).await {
            Ok(user) => {
                if let Err(err) = apply_roles(&state, &user.id, &roles).await {
                    skipped.push(json!({
                        "line": index + 1,
                        "reason": format!("账号已建但角色设置失败：{err:?}"),
                    }));
                    continue;
                }
                created += 1;
            }
            Err(err) => skipped.push(json!({
                "line": index + 1,
                "reason": err.to_string(),
            })),
        }
    }

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "user.import",
            target_type: "user",
            target_id: "",
            before_json: None,
            after_json: Some(&format!(
                r#"{{"created":{created},"skipped":{}}}"#,
                skipped.len()
            )),
        },
    )
    .await?;

    Ok(Json(json!({ "created": created, "skipped": skipped })))
}
