//! 资源管理 API：CRUD + 授权 + 密钥查看。

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::audit::{self, AuditInput};
use crate::crypto::{decrypt_secret, encrypt_secret};
use crate::grants::{self, Principal};
use crate::http::auth::{current_auth, AdminAuth};
use crate::http::error::{ApiError, ApiResult};
use crate::resources::{self, Resource};
use crate::state::Shared;

/// 每类资源对应哪三个权限（读 / 写 / 删）。
fn perms_for(kind: &str) -> (&'static str, &'static str, &'static str) {
    match kind {
        resources::KIND_PROVIDER => ("provider:read", "provider:write", "provider:delete"),
        resources::KIND_MODEL | resources::KIND_TIER_PROFILE => {
            ("model:read", "model:write", "model:delete")
        }
        resources::KIND_SKILL => ("skill:read", "skill:write", "skill:delete"),
        resources::KIND_PLUGIN => ("plugin:read", "plugin:write", "plugin:delete"),
        resources::KIND_MCP => ("mcp:read", "mcp:write", "mcp:write"),
        _ => ("system:read", "system:read", "system:read"),
    }
}

fn view(resource: &Resource) -> Value {
    let payload: Value =
        serde_json::from_str(&resource.payload_json).unwrap_or(serde_json::Value::Null);
    json!({
        "id": resource.id,
        "kind": resource.kind,
        "slug": resource.slug,
        "name": resource.name,
        "description": resource.description,
        "parentId": resource.parent_id,
        "enabled": resource.enabled,
        "payload": payload,
        "hasSecret": resource.has_secret,
        "activation": resource.activation,
        "createdAt": resource.created_at,
        "updatedAt": resource.updated_at,
    })
}

async fn load(state: &Shared, id: &str) -> ApiResult<Resource> {
    resources::get(&state.store, id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("资源不存在：{id}")))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub kind: Option<String>,
}

pub async fn list_resources(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;

    let kinds: Vec<&str> = match query.kind.as_deref().filter(|k| !k.is_empty()) {
        Some(kind) => {
            if !resources::is_known_kind(kind) {
                return Err(ApiError::BadRequest(format!("未知资源类型：{kind}")));
            }
            auth.require(perms_for(kind).0)?;
            vec![kind]
        }
        None => resources::KNOWN_KINDS
            .iter()
            .copied()
            .filter(|k| auth.has(perms_for(k).0))
            .collect(),
    };

    let mut items: Vec<Value> = Vec::new();
    for kind in kinds {
        for resource in resources::list(&state.store, &state.config.tenant_id, Some(kind)).await? {
            items.push(view(&resource));
        }
    }
    Ok(Json(json!({ "items": items, "total": items.len() })))
}

pub async fn get_resource(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    let resource = load(&state, &id).await?;
    auth.require(perms_for(&resource.kind).0)?;
    Ok(Json(view(&resource)))
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub kind: String,
    pub slug: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "parentId", alias = "parent_id")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    /// `optional`（默认）/ `default_on`：下发后客户端是否默认启用。
    #[serde(default)]
    pub activation: Option<String>,
}

pub async fn create_resource(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    auth.require(perms_for(&body.kind).1)?;

    let payload_json = match body.payload.as_ref() {
        Some(value) => serde_json::to_string(value).map_err(ApiError::internal)?,
        None => "{}".to_string(),
    };
    let encrypted = match body.secret.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(secret) => Some(encrypt_secret(secret).map_err(ApiError::internal)?),
        None => None,
    };

    let resource = resources::create(
        &state.store,
        &state.config.tenant_id,
        &body.kind,
        &body.slug,
        &body.name,
        &body.description,
        body.parent_id.as_deref(),
        &payload_json,
        encrypted.as_ref(),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    if body.enabled {
        resources::set_enabled(&state.store, &resource.id, true).await?;
    }
    if let Some(activation) = body.activation.as_deref() {
        resources::set_activation(&state.store, &resource.id, activation)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "resource.create",
            target_type: &resource.kind,
            target_id: &resource.id,
            before_json: None,
            after_json: Some(&payload_json),
        },
    )
    .await?;

    let resource = load(&state, &resource.id).await?;
    Ok(Json(view(&resource)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
    /// 传了就轮换；传空字符串等于清除。
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    /// `optional` / `default_on`。
    #[serde(default)]
    pub activation: Option<String>,
}

pub async fn update_resource(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    let existing = load(&state, &id).await?;
    auth.require(perms_for(&existing.kind).1)?;

    let payload_json = match body.payload.as_ref() {
        Some(value) => serde_json::to_string(value).map_err(ApiError::internal)?,
        None => existing.payload_json.clone(),
    };
    let name = body.name.clone().unwrap_or_else(|| existing.name.clone());
    let description = body
        .description
        .clone()
        .unwrap_or_else(|| existing.description.clone());

    resources::update_meta(&state.store, &id, &name, &description, &payload_json).await?;

    if let Some(secret) = body.secret.as_deref() {
        let encrypted = if secret.trim().is_empty() {
            None
        } else {
            Some(encrypt_secret(secret).map_err(ApiError::internal)?)
        };
        resources::set_secret(&state.store, &id, encrypted.as_ref()).await?;
    }
    if let Some(enabled) = body.enabled {
        resources::set_enabled(&state.store, &id, enabled).await?;
    }
    if let Some(activation) = body.activation.as_deref() {
        resources::set_activation(&state.store, &id, activation)
            .await
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "resource.update",
            target_type: &existing.kind,
            target_id: &id,
            before_json: Some(&existing.payload_json),
            after_json: Some(&payload_json),
        },
    )
    .await?;

    let updated = load(&state, &id).await?;
    Ok(Json(view(&updated)))
}

pub async fn delete_resource(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    let existing = load(&state, &id).await?;
    auth.require(perms_for(&existing.kind).2)?;

    resources::delete(&state.store, &id).await?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "resource.delete",
            target_type: &existing.kind,
            target_id: &id,
            before_json: Some(&existing.payload_json),
            after_json: None,
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn get_grants(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    let resource = load(&state, &id).await?;
    auth.require(perms_for(&resource.kind).0)?;

    let list = grants::grants(&state.store, &id).await?;
    let items: Vec<Value> = list
        .iter()
        .map(|p| match p {
            Principal::All => json!({ "type": "all", "id": "" }),
            Principal::Role(role_id) => json!({ "type": "role", "id": role_id }),
            Principal::User(user_id) => json!({ "type": "user", "id": user_id }),
        })
        .collect();
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct GrantItem {
    #[serde(rename = "type")]
    pub principal_type: String,
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct GrantsBody {
    #[serde(default)]
    pub items: Vec<GrantItem>,
}

pub async fn replace_grants(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<GrantsBody>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    let resource = load(&state, &id).await?;
    auth.require(perms_for(&resource.kind).1)?;

    let mut principals = Vec::with_capacity(body.items.len());
    for item in &body.items {
        principals.push(
            Principal::parse(&item.principal_type, &item.id)
                .map_err(|e| ApiError::BadRequest(e.to_string()))?,
        );
    }

    grants::replace_grants(&state.store, &id, &principals)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "resource.grants",
            target_type: &resource.kind,
            target_id: &id,
            before_json: None,
            after_json: Some(&serde_json::to_string(&body.items).unwrap_or_default()),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

/// 测试连接：拿 baseUrl + key 探一次 `{baseUrl}/models`。
///
/// 只回答「能不能连、看到几个模型」，不落库、不写密钥。
/// 失败也走 200 + `ok:false`，让前端把原因显示在表单里，而不是弹一个 500。
#[derive(Debug, Deserialize)]
pub struct TestProviderBody {
    #[serde(rename = "baseUrl", alias = "base_url")]
    pub base_url: String,
    #[serde(default, rename = "apiKey", alias = "api_key")]
    pub api_key: String,
}

pub async fn test_provider(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<TestProviderBody>,
) -> ApiResult<Json<Value>> {
    let auth: AdminAuth = current_auth(&state, &headers).await?;
    auth.require("provider:write")?;

    let raw = body.base_url.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(raw)
        .map_err(|_| ApiError::BadRequest("baseUrl 不是合法 URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ApiError::BadRequest("baseUrl 只支持 http/https".into()));
    }
    // 云元数据地址是 SSRF 的经典跳板，管理端不该被当成跳板用。
    if parsed.host_str() == Some("169.254.169.254") {
        return Err(ApiError::BadRequest("不允许探测云元数据地址".into()));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(ApiError::internal)?;
    let mut req = client.get(format!("{raw}/models"));
    if !body.api_key.trim().is_empty() {
        req = req.bearer_auth(body.api_key.trim());
    }

    let out = match req.send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                json!({"ok": false, "message": format!("服务商返回 HTTP {status}")})
            } else {
                let text = resp.text().await.unwrap_or_default();
                // 顺手把模型清单带回去：新建服务商时直接勾选要启用的模型，
                // 省掉「先建服务商、再逐个手敲模型 slug」这一步。
                let models: Vec<String> = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v.get("data").and_then(|d| d.as_array()).map(|arr| {
                            arr.iter()
                                .filter_map(|m| m.get("id").and_then(|i| i.as_str()))
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                        })
                    })
                    .unwrap_or_default();
                json!({"ok": true, "modelCount": models.len(), "models": models})
            }
        }
        Err(err) => {
            let msg = if err.is_timeout() {
                "连接超时（8 秒）".to_string()
            } else if err.is_connect() {
                "连不上：域名解析失败或网络不通".to_string()
            } else {
                format!("连接失败：{err}")
            };
            json!({"ok": false, "message": msg})
        }
    };
    Ok(Json(out))
}

/// 查看明文密钥：单独一个权限（`provider:reveal`），并且每次都进审计。
pub async fn reveal_secret(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth: AdminAuth = current_auth(&state, &headers).await?;
    auth.require("provider:reveal")?;

    let resource = load(&state, &id).await?;
    let Some(encrypted) = resources::secret(&state.store, &id).await? else {
        return Err(ApiError::NotFound("该资源没有配置密钥".to_string()));
    };
    let plain = decrypt_secret(&encrypted).map_err(ApiError::internal)?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "resource.reveal",
            target_type: &resource.kind,
            target_id: &id,
            before_json: None,
            after_json: None,
        },
    )
    .await?;

    Ok(Json(json!({ "secret": plain })))
}

// ---- 技能包版本 ----

fn version_view(v: &crate::versions::SkillVersion) -> Value {
    json!({
        "id": v.id,
        "version": v.version,
        "hash": v.dir_hash,
        "sizeBytes": v.size_bytes,
        "uploadedBy": v.uploaded_by,
        "createdAt": v.created_at,
        "isCurrent": v.is_current,
    })
}

/// 版本类操作按资源类型落到不同权限域（技能 / 插件）。
async fn version_perms(
    state: &Shared,
    auth: &AdminAuth,
    resource_id: &str,
    action: &str,
) -> ApiResult<()> {
    let resource = load(state, resource_id).await?;
    let domain = if resource.kind == resources::KIND_PLUGIN {
        "plugin"
    } else {
        "skill"
    };
    auth.require(&format!("{domain}:{action}"))
}

pub async fn list_versions(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    version_perms(&state, &auth, &id, "read").await?;
    let items: Vec<Value> = crate::versions::list_versions(&state.store, &id)
        .await?
        .iter()
        .map(version_view)
        .collect();
    Ok(Json(json!({ "items": items, "total": items.len() })))
}

#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    pub version: String,
}

/// 上传 zip：请求体就是原始字节，版本号走查询串。
pub async fn upload_version(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<VersionQuery>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    version_perms(&state, &auth, &id, "write").await?;

    let version = crate::versions::add_version(
        &state.store,
        &state.config.data_dir,
        &id,
        &query.version,
        &body,
        Some(&auth.user.id),
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "skill.version.upload",
            target_type: "skill",
            target_id: &id,
            before_json: None,
            after_json: Some(&format!(
                r#"{{"version":"{}","hash":"{}","size":{}}}"#,
                version.version, version.dir_hash, version.size_bytes
            )),
        },
    )
    .await?;

    Ok(Json(version_view(&version)))
}

pub async fn publish_version(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path((id, version_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    version_perms(&state, &auth, &id, "publish").await?;

    crate::versions::publish(&state.store, &id, &version_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "skill.version.publish",
            target_type: "skill",
            target_id: &id,
            before_json: None,
            after_json: Some(&format!(r#"{{"versionId":"{version_id}"}}"#)),
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn delete_version(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path((_id, version_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let auth = current_auth(&state, &headers).await?;
    version_perms(&state, &auth, &_id, "write").await?;

    let removed = crate::versions::delete_version(&state.store, &state.config.data_dir, &version_id)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !removed {
        return Err(ApiError::NotFound("版本不存在".to_string()));
    }

    audit::record(
        &state.store,
        &state.config.tenant_id,
        AuditInput {
            actor_user_id: Some(&auth.user.id),
            actor_name: &auth.user.display_name,
            action: "skill.version.delete",
            target_type: "skill",
            target_id: &version_id,
            before_json: None,
            after_json: None,
        },
    )
    .await?;

    Ok(Json(json!({ "ok": true })))
}
