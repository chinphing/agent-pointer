//! 面向 server / 客户端的 partner 接口（Bearer access token）。

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::credentials;
use crate::db::{now_ts, sha256_hex};
use crate::grants;
use crate::http::error::{ApiError, ApiResult};
use crate::oauth::token::{month_prefix, today_string};
use crate::resources::{self, KIND_PLUGIN, KIND_SKILL};
use crate::sessions;
use crate::state::Shared;
use crate::usage;
use crate::users::{self, User};
use crate::versions;

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

// ---- 技能下发 ----

/// 技能与插件共用一套下发逻辑，只是资源类型与 URL 段不同。
fn package_path(kind: &str) -> &'static str {
    if kind == KIND_PLUGIN {
        "plugins"
    } else {
        "skills"
    }
}

async fn list_packages(
    state: &Shared,
    user: &User,
    kind: &str,
) -> ApiResult<Value> {
    let visible: std::collections::HashSet<String> =
        grants::visible_resource_ids(&state.store, &state.config.tenant_id, &user.id)
            .await?
            .into_iter()
            .collect();

    let segment = package_path(kind);
    let mut items: Vec<Value> = Vec::new();
    for resource in resources::list(&state.store, &state.config.tenant_id, Some(kind)).await? {
        if !resource.enabled || !visible.contains(&resource.id) {
            continue;
        }
        let Some(current) = resource.current_version_id.as_deref() else {
            continue;
        };
        let Some(version) = versions::get_version(&state.store, current).await? else {
            continue;
        };
        // 插件必须带 plugin_id：客户端要按它建目录、核对包内 id。
        let plugin_id = if kind == KIND_PLUGIN {
            serde_json::from_str::<Value>(&resource.payload_json)
                .ok()
                .and_then(|v| {
                    v.get("pluginId")
                        .or_else(|| v.get("plugin_id"))
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                })
        } else {
            None
        };
        items.push(json!({
            "slug": resource.slug,
            "name": resource.name,
            "pluginId": plugin_id,
            "version": version.version,
            "hash": version.dir_hash,
            "sizeBytes": version.size_bytes,
            "url": format!(
                "/auth/partner/{}/{}/{}/download",
                segment, resource.slug, version.version
            ),
        }));
    }
    items.sort_by(|a, b| a["slug"].as_str().cmp(&b["slug"].as_str()));

    // 目录哈希：客户端据此判断「要不要重新同步」，算法固定且顺序无关。
    let mut keys: Vec<String> = items
        .iter()
        .map(|item| {
            format!(
                "{}@{}#{}",
                item["slug"].as_str().unwrap_or_default(),
                item["version"].as_str().unwrap_or_default(),
                item["hash"].as_str().unwrap_or_default()
            )
        })
        .collect();
    keys.sort();
    let catalog_hash = sha256_hex(keys.join("\n").as_bytes())[..16].to_string();

    Ok(json!({ segment: items, "catalogHash": catalog_hash }))
}

async fn download_package(
    state: &Shared,
    user: &User,
    kind: &str,
    slug: &str,
    version: &str,
) -> ApiResult<Response> {
    let resource = resources::find_by_slug(&state.store, &state.config.tenant_id, kind, slug)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("资源不存在：{slug}")))?;

    if !resource.enabled || !grants::can_see(&state.store, &resource.id, &user.id).await? {
        return Err(ApiError::NotFound(format!("资源不存在：{slug}")));
    }

    let version = versions::find_version(&state.store, &resource.id, version)
        .await?
        .ok_or_else(|| ApiError::NotFound("版本不存在".to_string()))?;

    let bytes = versions::read_blob(&state.config.data_dir, &version.id, &state.store)
        .await
        .map_err(ApiError::internal)?;

    let filename = format!("{}-{}.zip", resource.slug, version.version);
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

/// `GET /auth/partner/skills`
pub async fn skills(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    Ok(Json(list_packages(&state, &user, KIND_SKILL).await?))
}

/// `GET /auth/partner/plugins`
pub async fn plugins(
    State(state): State<Shared>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let user = bearer_user(&state, &headers).await?;
    Ok(Json(list_packages(&state, &user, KIND_PLUGIN).await?))
}

/// `GET /auth/partner/skills/:slug/:version/download`
pub async fn download_skill(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path((slug, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let user = bearer_user(&state, &headers).await?;
    download_package(&state, &user, KIND_SKILL, &slug, &version).await
}

/// `GET /auth/partner/plugins/:slug/:version/download`
pub async fn download_plugin(
    State(state): State<Shared>,
    headers: HeaderMap,
    Path((slug, version)): Path<(String, String)>,
) -> ApiResult<Response> {
    let user = bearer_user(&state, &headers).await?;
    download_package(&state, &user, KIND_PLUGIN, &slug, &version).await
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
        state_with_user_in(std::path::PathBuf::from("admin/data")).await
    }

    async fn state_with_user_in(data_dir: std::path::PathBuf) -> (Shared, String) {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let uid = users::create(&store, DEFAULT_TENANT, "bob", "鲍勃", "pw-123456")
            .await
            .expect("user")
            .id;
        let shared = Arc::new(crate::state::AppState {
            store,
            config: AdminConfig {
                data_dir,
                ..Default::default()
            },
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
    async fn skills_listing_requires_publish_and_visibility() {
        use crate::grants::Principal;
        use crate::resources;
        use crate::versions;

        let data_dir = versions::temp_data_dir("partner");
        let (state, uid) = state_with_user_in(data_dir.clone()).await;
        let skill = resources::create(
            &state.store,
            DEFAULT_TENANT,
            resources::KIND_SKILL,
            "travel",
            "差旅报销",
            "",
            None,
            "{}",
            None,
        )
        .await
        .expect("skill");

        let zip = versions::build_test_zip("# travel", &[]);
        let version = versions::add_version(
            &state.store,
            &data_dir,
            &skill.id,
            "1.0.0",
            &zip,
            Some(&uid),
        )
        .await
        .expect("version");

        let access = sessions::create(&state.store, &uid, sessions::KIND_ACCESS, 3600)
            .await
            .expect("access");
        let headers = bearer(&access);

        // 未授权：看不到
        let list = skills(State(state.clone()), headers.clone()).await.expect("list");
        assert!(list.0["skills"].as_array().expect("arr").is_empty());

        // 授权了但没发布（enabled 仍为 0）：还是看不到
        grants::replace_grants(&state.store, &skill.id, &[Principal::All])
            .await
            .expect("grant");
        let list = skills(State(state.clone()), headers.clone()).await.expect("list");
        assert!(list.0["skills"].as_array().expect("arr").is_empty());

        // 发布后可见
        versions::publish(&state.store, &skill.id, &version.id)
            .await
            .expect("publish");
        let list = skills(State(state.clone()), headers.clone()).await.expect("list");
        let arr = list.0["skills"].as_array().expect("arr");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["version"], "1.0.0");
        assert_eq!(arr[0]["slug"], "travel");
        assert!(list.0["catalogHash"].as_str().is_some());

        // 下载拿到原始字节
        let response = download_skill(
            State(state.clone()),
            headers,
            Path(("travel".to_string(), "1.0.0".to_string())),
        )
        .await
        .expect("download");
        assert_eq!(response.status(), 200);

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[tokio::test]
    async fn balance_reports_quota_block() {
        let (state, uid) = state_with_user().await;
        usage::record(
            &state.store,
            DEFAULT_TENANT,
            &uid,
            "qwen",
            &today_string(now_ts()),
            100,
            0,
        )
        .await
        .expect("usage");
        usage::set_quota(
            &state.store,
            DEFAULT_TENANT,
            &uid,
            Some(50),
            usage::OVER_ACTION_BLOCK,
        )
        .await
        .expect("quota");

        let access = sessions::create(&state.store, &uid, sessions::KIND_ACCESS, 3600)
            .await
            .expect("access");
        let body = balance(State(state.clone()), bearer(&access))
            .await
            .expect("balance");
        assert_eq!(
            body.0["token_quota_exhausted"], true,
            "block 策略超限必须让客户端 gate 拦住"
        );
    }

    #[tokio::test]
    async fn balance_does_not_block_on_warn() {
        let (state, uid) = state_with_user().await;
        usage::record(
            &state.store,
            DEFAULT_TENANT,
            &uid,
            "qwen",
            &today_string(now_ts()),
            100,
            0,
        )
        .await
        .expect("usage");
        usage::set_quota(
            &state.store,
            DEFAULT_TENANT,
            &uid,
            Some(50),
            usage::OVER_ACTION_WARN,
        )
        .await
        .expect("quota");

        let access = sessions::create(&state.store, &uid, sessions::KIND_ACCESS, 3600)
            .await
            .expect("access");
        let body = balance(State(state.clone()), bearer(&access))
            .await
            .expect("balance");
        assert_eq!(body.0["token_quota_exhausted"], false, "warn 只提示不拦");
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
