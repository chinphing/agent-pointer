//! 客户端自动更新：版本清单、产物存储、下发。
//!
//! 契约与 `tauri-plugin-updater` 2.x 原生静态 manifest 一致，客户端
//! `src-tauri/src/updater_commands.rs` 直接把 `{api_base}` 拼成
//! `/api/updates/latest?target=…&arch=…&current_version=…` 后交给插件解析：
//!
//! ```json
//! { "version": "1.2.3", "notes": "…", "pub_date": "2026-07-12T04:00:00Z",
//!   "platforms": { "darwin-aarch64": { "url": "…", "signature": "<.sig 全文>" } } }
//! ```
//!
//! 三条硬约束：
//! 1. `signature` 必须内联全文 —— 插件只认内联，给 URL 会验签失败；
//! 2. 下载链路不能做任何内容变换（gzip/转码），否则签名对不上；
//! 3. 没有可用产物时返回 204，不能返回空 platforms —— 客户端会报
//!    `TargetsNotFound` 而不是「已是最新」。

use anyhow::Result;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use tower::ServiceExt;

use crate::db::{now_ts, AdminStore};
use crate::http::error::{ApiError, ApiResult};
use crate::state::Shared;

/// **自动更新包**：Tauri updater 产物（`.tar.gz` + `.sig`），只进
/// `/api/updates/latest` 的清单，客户端启动时自动更新用。
///
/// 产物类型 → manifest 里的 platform key。与官方实现保持一致：
/// macOS 一份产物同时供 aarch64 / x86_64，Windows 同理。
const UPDATER_KINDS: &[(&str, &[&str])] = &[
    ("macos_updater", &["darwin-aarch64", "darwin-x86_64"]),
    ("windows_updater", &["windows-x86_64", "windows-i686"]),
    ("linux_appimage_updater", &["linux-x86_64"]),
];

/// **安装包**：用户在下载页手动下载的完整安装程序，与官网下载页的四种格式对齐
/// （`.dmg` / `.msi` / `.deb` / `.AppImage`）。
///
/// 与更新包是两回事：更新包是给 updater 用的差量/全量包，装不上；下载页给错了
/// 文件，用户拿到的是一个 tar.gz 而不是能双击安装的程序。
const INSTALLER_KINDS: &[&str] = &[
    "macos_dmg",
    "windows_msi",
    "windows_nsis",
    "linux_deb",
    "linux_appimage",
];

/// 单个安装包 / 更新包的上限。
///
/// 桌面安装包通常 80–200 MB，`.AppImage` 还要更大。**必须显式设置**：axum 默认
/// body 上限只有 2 MiB，超了 multipart 会直接解析失败，前端只会看到一句
/// 「Error parsing multipart/form-data request」，看不出是文件太大。
pub const MAX_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;

pub fn kind_platform_keys(kind: &str) -> &'static [&'static str] {
    UPDATER_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, keys)| *keys)
        .unwrap_or(&[])
}

/// 上传时允许的全部类型：更新包 + 安装包。
pub fn known_kinds() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = UPDATER_KINDS.iter().map(|(k, _)| *k).collect();
    all.extend_from_slice(INSTALLER_KINDS);
    all
}

pub fn updater_kinds() -> Vec<&'static str> {
    UPDATER_KINDS.iter().map(|(k, _)| *k).collect()
}

pub fn installer_kinds() -> Vec<&'static str> {
    INSTALLER_KINDS.to_vec()
}

/// 这个产物是不是安装包（下载页只展示安装包）。
pub fn is_installer_kind(kind: &str) -> bool {
    INSTALLER_KINDS.contains(&kind)
}

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub id: String,
    pub version: String,
    pub notes: String,
    pub published: bool,
    pub published_at: Option<i64>,
    pub created_at: i64,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Artifact {
    pub id: String,
    pub kind: String,
    pub filename: String,
    pub size: i64,
    /// `.sig` 是否已填。列表接口不回全文，避免把签名刷到列表里。
    pub has_signature: bool,
    pub created_at: i64,
    /// 以下三个只在服务端内部用（复用、删除前查引用），不进列表接口。
    #[serde(skip)]
    pub stored_as: String,
    #[serde(skip)]
    pub signature: String,
    #[serde(skip)]
    pub sha256: String,
}

fn row_to_artifact(row: &sqlx::any::AnyRow) -> Artifact {
    let sig: String = row.try_get("signature").unwrap_or_default();
    Artifact {
        id: row.try_get("id").unwrap_or_default(),
        kind: row.try_get("kind").unwrap_or_default(),
        filename: row.try_get("filename").unwrap_or_default(),
        size: row.try_get("size").unwrap_or(0),
        has_signature: !sig.trim().is_empty(),
        created_at: row.try_get("created_at").unwrap_or(0),
        stored_as: row.try_get("stored_as").unwrap_or_default(),
        signature: sig,
        sha256: row.try_get("sha256").unwrap_or_default(),
    }
}

async fn artifacts_for(store: &AdminStore, release_id: &str) -> Result<Vec<Artifact>> {
    let sql = store.sql(
        "SELECT id, kind, filename, size, signature, created_at, stored_as, sha256
         FROM app_release_artifacts WHERE release_id = ? ORDER BY kind",
    );
    let rows = sqlx::query::<sqlx::Any>(&sql)
        .bind(release_id)
        .fetch_all(store.pool())
        .await?;
    Ok(rows.iter().map(row_to_artifact).collect())
}

pub async fn list_releases(store: &AdminStore, tenant_id: &str) -> Result<Vec<Release>> {
    let sql = store.sql(
        "SELECT id, version, notes, published, published_at, created_at
         FROM app_releases WHERE tenant_id = ? ORDER BY created_at DESC",
    );
    let rows = sqlx::query::<sqlx::Any>(&sql)
        .bind(tenant_id)
        .fetch_all(store.pool())
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.try_get("id")?;
        out.push(Release {
            artifacts: artifacts_for(store, &id).await?,
            id,
            version: row.try_get("version")?,
            notes: row.try_get("notes")?,
            published: row.try_get::<i64, _>("published").unwrap_or(0) != 0,
            published_at: row.try_get("published_at").ok(),
            created_at: row.try_get("created_at").unwrap_or(0),
        });
    }
    Ok(out)
}

pub async fn create_release(
    store: &AdminStore,
    tenant_id: &str,
    version: &str,
    notes: &str,
) -> Result<Release> {
    let version = version.trim().trim_start_matches('v');
    if parse_semver(version).is_none() {
        anyhow::bail!("版本号格式应为 1.2.3（可带 -beta.1 后缀）");
    }
    let existing = store.sql("SELECT id FROM app_releases WHERE tenant_id = ? AND version = ?");
    if sqlx::query::<sqlx::Any>(&existing)
        .bind(tenant_id)
        .bind(version)
        .fetch_optional(store.pool())
        .await?
        .is_some()
    {
        anyhow::bail!("版本 {version} 已存在");
    }
    let id = crate::db::random_id();
    let now = now_ts();
    let sql = store.sql(
        "INSERT INTO app_releases (id, tenant_id, version, notes, published, published_at, created_at)
         VALUES (?, ?, ?, ?, 0, NULL, ?)",
    );
    sqlx::query::<sqlx::Any>(&sql)
        .bind(&id)
        .bind(tenant_id)
        .bind(version)
        .bind(notes.trim())
        .bind(now)
        .execute(store.pool())
        .await?;
    Ok(Release {
        id,
        version: version.to_string(),
        notes: notes.trim().to_string(),
        published: false,
        published_at: None,
        created_at: now,
        artifacts: Vec::new(),
    })
}

pub async fn delete_release(store: &AdminStore, tenant_id: &str, id: &str) -> Result<()> {
    let sql = store.sql("DELETE FROM app_releases WHERE id = ? AND tenant_id = ?");
    sqlx::query::<sqlx::Any>(&sql)
        .bind(id)
        .bind(tenant_id)
        .execute(store.pool())
        .await?;
    let sql = store.sql("DELETE FROM app_release_artifacts WHERE release_id = ?");
    sqlx::query::<sqlx::Any>(&sql)
        .bind(id)
        .execute(store.pool())
        .await?;
    Ok(())
}

pub async fn set_published(
    store: &AdminStore,
    tenant_id: &str,
    id: &str,
    published: bool,
) -> Result<()> {
    let sql = store.sql(
        "UPDATE app_releases SET published = ?, published_at = ? WHERE id = ? AND tenant_id = ?",
    );
    sqlx::query::<sqlx::Any>(&sql)
        .bind(if published { 1 } else { 0 })
        .bind(if published { Some(now_ts()) } else { None })
        .bind(id)
        .bind(tenant_id)
        .execute(store.pool())
        .await?;
    Ok(())
}

pub async fn add_artifact(
    store: &AdminStore,
    release_id: &str,
    kind: &str,
    filename: &str,
    stored_as: &str,
    size: i64,
    signature: &str,
    sha256: &str,
) -> Result<String> {
    let id = crate::db::random_id();
    let sql = store.sql(
        "INSERT INTO app_release_artifacts
           (id, release_id, kind, filename, stored_as, size, signature, created_at, sha256)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    );
    sqlx::query::<sqlx::Any>(&sql)
        .bind(&id)
        .bind(release_id)
        .bind(kind)
        .bind(filename)
        .bind(stored_as)
        .bind(size)
        .bind(signature.trim())
        .bind(now_ts())
        .bind(sha256)
        .execute(store.pool())
        .await?;
    Ok(id)
}

/// 内容相同的产物只存一份：按 sha256 找已存在的那份磁盘文件。
///
/// 只有 macOS 变了的时候，Windows/Linux 的包跟上一版逐字节相同，没必要再存一份。
pub async fn find_stored_by_sha(store: &AdminStore, sha256: &str) -> Result<Option<String>> {
    if sha256.trim().is_empty() {
        return Ok(None);
    }
    let sql = store.sql(
        "SELECT stored_as FROM app_release_artifacts WHERE sha256 = ? LIMIT 1",
    );
    let row = sqlx::query::<sqlx::Any>(&sql)
        .bind(sha256)
        .fetch_optional(store.pool())
        .await?;
    Ok(row.and_then(|r| r.try_get::<String, _>("stored_as").ok()))
}

/// 还有没有别的产物记录引用同一个磁盘文件。删除前必须查，否则复用会把别人的包删掉。
pub async fn stored_as_referenced(store: &AdminStore, stored_as: &str) -> Result<bool> {
    let sql = store.sql(
        "SELECT COUNT(*) AS n FROM app_release_artifacts WHERE stored_as = ?",
    );
    let row = sqlx::query::<sqlx::Any>(&sql)
        .bind(stored_as)
        .fetch_one(store.pool())
        .await?;
    let n: i64 = row.try_get("n").unwrap_or(0);
    Ok(n > 0)
}

/// 把 `from` 版本里的产物**按引用**复制到 `to` 版本，只补 `to` 还没有的 kind。
///
/// 不复制文件，只多一条指向同一 `stored_as` 的记录 —— 这就是「相同的包直接复用」。
/// 返回真正复制过去的 kind 列表。
pub async fn reuse_artifacts(
    store: &AdminStore,
    to_release: &str,
    from_release: &str,
) -> Result<Vec<String>> {
    let src = artifacts_for(store, from_release).await?;
    let mut copied = Vec::new();
    for a in src {
        let sql = store.sql(
            "SELECT COUNT(*) AS n FROM app_release_artifacts WHERE release_id = ? AND kind = ?",
        );
        let row = sqlx::query::<sqlx::Any>(&sql)
            .bind(to_release)
            .bind(&a.kind)
            .fetch_one(store.pool())
            .await?;
        let n: i64 = row.try_get("n").unwrap_or(0);
        if n > 0 {
            continue; // 目标版本已有同类型产物，不覆盖
        }
        add_artifact(
            store,
            to_release,
            &a.kind,
            &a.filename,
            &a.stored_as,
            a.size,
            &a.signature,
            &a.sha256,
        )
        .await?;
        copied.push(a.kind.clone());
    }
    Ok(copied)
}

pub async fn delete_artifact(store: &AdminStore, release_id: &str, kind: &str) -> Result<()> {
    let sql = store.sql("DELETE FROM app_release_artifacts WHERE release_id = ? AND kind = ?");
    sqlx::query::<sqlx::Any>(&sql)
        .bind(release_id)
        .bind(kind)
        .execute(store.pool())
        .await?;
    Ok(())
}

/// 只解析 `主.次.补丁`，忽略 `-beta.1` 这类预发布后缀 —— 与官方
/// `semver_is_newer` 同口径（客户端仍会用真正的 semver 再判一次）。
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(release: &str, current: &str) -> bool {
    match (parse_semver(release), parse_semver(current)) {
        (Some(r), Some(c)) => r > c,
        // 当前版本解析不了就按「有更新」处理，让客户端自己去判。
        (Some(_), None) => true,
        _ => false,
    }
}

/// 组装 manifest。没有任何带签名的产物时返回 None（调用方回 204）。
pub fn build_manifest(
    version: &str,
    notes: &str,
    published_at: Option<i64>,
    base_url: &str,
    artifacts: &[(String, String, String, String)],
) -> Option<Value> {
    // artifacts: (id, kind, filename, signature)
    let mut platforms = serde_json::Map::new();
    for (id, kind, _filename, signature) in artifacts {
        if signature.trim().is_empty() {
            continue;
        }
        for key in kind_platform_keys(kind) {
            platforms.insert(
                (*key).to_string(),
                json!({
                    "url": format!("{}/api/downloads/files/{}", base_url.trim_end_matches('/'), id),
                    "signature": signature,
                }),
            );
        }
    }
    if platforms.is_empty() {
        return None;
    }
    let pub_date = published_at
        .and_then(time_like_iso)
        .unwrap_or_default();
    Some(json!({
        "version": version,
        "notes": notes,
        "pub_date": pub_date,
        "platforms": platforms,
    }))
}

/// unix 秒 → `2026-07-12T04:00:00Z`。不引 chrono，手工换算。
fn time_like_iso(ts: i64) -> Option<String> {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    ))
}

/// Howard Hinnant 的 days→civil 算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn public_base(state: &Shared, headers: &HeaderMap) -> String {
    if let Some(base) = state.config.public_base.as_deref() {
        if !base.trim().is_empty() {
            return base.trim_end_matches('/').to_string();
        }
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1:8788");
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    format!("{proto}://{host}")
}

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub current_version: Option<String>,
}

/// `GET /api/downloads/releases` —— 公开的版本 + 产物清单（下载页用）。
///
/// 与 `/api/updates/latest` 的区别：那个只回**最新一版**、且跳过未签名产物
/// （更新器装不了没签名的包）。下载页要能列出所有平台的包，所以这里不要求签名。
pub async fn downloads_list(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<DownloadsQuery>,
) -> ApiResult<Json<Value>> {
    let limit = q.limit.unwrap_or(5).clamp(1, 20);
    let sql = state.store.sql(
        "SELECT id, version, notes, published_at
           FROM app_releases
          WHERE tenant_id = ? AND published = 1
          ORDER BY published_at DESC, created_at DESC
          LIMIT ?",
    );
    let rows = sqlx::query::<sqlx::Any>(&sql)
        .bind(&state.config.tenant_id)
        .bind(limit)
        .fetch_all(state.store.pool())
        .await
        .map_err(ApiError::internal)?;

    let base = public_base(&state, &headers);
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: String = row.try_get("id").map_err(ApiError::internal)?;
        let version: String = row.try_get("version").map_err(ApiError::internal)?;
        let notes: String = row.try_get("notes").unwrap_or_default();
        let published_at: Option<i64> = row.try_get("published_at").ok();
        let artifacts = artifacts_for(&state.store, &id)
            .await
            .map_err(ApiError::internal)?;
        let list: Vec<Value> = artifacts
            .iter()
            // 下载页只给安装包：更新包是给客户端 updater 用的，装不上。
            .filter(|a| is_installer_kind(&a.kind))
            .map(|a| {
                json!({
                    "id": a.id,
                    "kind": a.kind,
                    "filename": a.filename,
                    "size": a.size,
                    "downloadUrl": format!(
                        "{}/api/downloads/files/{}",
                        base.trim_end_matches('/'),
                        a.id
                    ),
                })
            })
            .collect();
        items.push(json!({
            "version": version,
            "notes": notes,
            "publishedAt": published_at,
            "artifacts": list,
        }));
    }
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
pub struct DownloadsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `GET /api/updates/latest` —— 公开，客户端不带鉴权。
pub async fn updates_latest(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(q): Query<UpdateQuery>,
) -> Response {
    match latest_response(&state, &headers, q).await {
        Ok(Some(v)) => Json(v).into_response(),
        // 无更新：204，客户端 check() 返回 None。
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::internal(e).into_response(),
    }
}

async fn latest_response(
    state: &Shared,
    headers: &HeaderMap,
    q: UpdateQuery,
) -> Result<Option<Value>> {
    let store = &state.store;
    let sql = store.sql(
        "SELECT id, version, notes, published_at FROM app_releases
         WHERE tenant_id = ? AND published = 1
         ORDER BY published_at DESC, created_at DESC",
    );
    let row = sqlx::query::<sqlx::Any>(&sql)
        .bind(&state.config.tenant_id)
        .fetch_optional(store.pool())
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: String = row.try_get("id")?;
    let version: String = row.try_get("version")?;
    let notes: String = row.try_get("notes").unwrap_or_default();
    let published_at: Option<i64> = row.try_get("published_at").ok();

    if let Some(current) = q.current_version.as_deref() {
        if !current.trim().is_empty() && !is_newer(&version, current.trim()) {
            return Ok(None);
        }
    }

    let sql = store.sql(
        "SELECT id, kind, filename, signature FROM app_release_artifacts WHERE release_id = ?",
    );
    let rows = sqlx::query::<sqlx::Any>(&sql)
        .bind(&id)
        .fetch_all(store.pool())
        .await?;
    let artifacts: Vec<(String, String, String, String)> = rows
        .iter()
        .map(|r| {
            (
                r.try_get::<String, _>("id").unwrap_or_default(),
                r.try_get::<String, _>("kind").unwrap_or_default(),
                r.try_get::<String, _>("filename").unwrap_or_default(),
                r.try_get::<String, _>("signature").unwrap_or_default(),
            )
        })
        .collect();

    let base = public_base(state, headers);
    Ok(build_manifest(&version, &notes, published_at, &base, &artifacts))
}

/// `GET /api/downloads/files/:id` —— 公开，直接给二进制。
///
/// 用 `ServeFile` 而不是读进内存：安装包上百 MB，且它自带 Content-Length
/// 与 Range 支持。**不能**在这里加任何压缩/转码 —— 签名覆盖的是原始字节。
pub async fn download_file(
    State(state): State<Shared>,
    Path(id): Path<String>,
    req: axum::extract::Request,
) -> Response {
    let store = &state.store;
    let sql = store.sql("SELECT stored_as, filename FROM app_release_artifacts WHERE id = ?");
    let row = match sqlx::query::<sqlx::Any>(&sql)
        .bind(&id)
        .fetch_optional(store.pool())
        .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "file_not_found").into_response(),
        Err(e) => return ApiError::internal(e).into_response(),
    };
    let stored_as: String = row.try_get("stored_as").unwrap_or_default();
    let filename: String = row.try_get("filename").unwrap_or_default();
    let path = state.config.data_dir.join(&stored_as);
    if !path.exists() {
        return (StatusCode::NOT_FOUND, "file_missing").into_response();
    }

    let mut response = match tower_http::services::ServeFile::new(&path).oneshot(req).await {
        Ok(resp) => resp.into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "read_failed").into_response(),
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!(
        "attachment; filename=\"{}\"",
        filename.replace('"', "")
    )) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    response
}

// ---------- 管理端 ----------

pub async fn admin_list(State(state): State<Shared>) -> ApiResult<Json<Value>> {

    let rows = list_releases(&state.store, &state.config.tenant_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({
        "items": rows,
        "kinds": known_kinds(),
        // 前端分两组渲染：安装包（下载页展示）与更新包（客户端自动更新）。
        "installerKinds": installer_kinds(),
        "updaterKinds": updater_kinds(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct CreateReleaseBody {
    pub version: String,
    #[serde(default)]
    pub notes: String,
}

pub async fn admin_create(
    State(state): State<Shared>,
    Json(body): Json<CreateReleaseBody>,
) -> ApiResult<Json<Value>> {

    let release = create_release(
        &state.store,
        &state.config.tenant_id,
        &body.version,
        &body.notes,
    )
    .await
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(serde_json::to_value(release).map_err(ApiError::internal)?))
}

pub async fn admin_delete(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {

    delete_release(&state.store, &state.config.tenant_id, &id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct PublishBody {
    pub published: bool,
}

pub async fn admin_publish(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<PublishBody>,
) -> ApiResult<Json<Value>> {

    if body.published {
        // 发布前必须至少有一份带签名的产物，否则客户端只会拿到 204。
        let artifacts = artifacts_for(&state.store, &id)
            .await
            .map_err(ApiError::internal)?;
        if !artifacts.iter().any(|a| a.has_signature) {
            return Err(ApiError::BadRequest(
                "还没有任何带签名的安装包，发布后客户端拿不到更新".to_string(),
            ));
        }
    }
    set_published(&state.store, &state.config.tenant_id, &id, body.published)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/admin/releases/:id/artifacts` —— multipart：
/// `kind`（文本）+ `signature`（文本，.sig 全文）+ `file`（安装包）。
pub async fn admin_upload_artifact(
    State(state): State<Shared>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {

    let mut kind = String::new();
    let mut signature = String::new();
    let mut filename = String::new();
    let mut bytes: Option<Vec<u8>> = None;

    // 超限时 multer 只会吐一句「Error parsing multipart/form-data request」，
    // 看不出是文件太大；这里翻译成人话，带上上限。
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        ApiError::BadRequest(format!(
            "上传失败：{}（单个文件上限 {} MB）",
            e,
            MAX_ARTIFACT_BYTES / 1024 / 1024
        ))
    })? {
        match field.name().unwrap_or_default() {
            "kind" => kind = field.text().await.map_err(ApiError::internal)?,
            "signature" => signature = field.text().await.map_err(ApiError::internal)?,
            "file" => {
                filename = field.file_name().unwrap_or("artifact").to_string();
                bytes = Some(field.bytes().await.map_err(ApiError::internal)?.to_vec());
            }
            _ => {}
        }
    }

    let kind = kind.trim();
    if !known_kinds().contains(&kind) {
        return Err(ApiError::BadRequest(format!(
            "未知的产物类型：{kind}（可选：{}）",
            known_kinds().join(" / ")
        )));
    }
    let Some(bytes) = bytes else {
        return Err(ApiError::BadRequest("缺少安装包文件".to_string()));
    };
    // 只有更新包必须带签名：客户端 updater 验签失败会直接拒绝安装。
    // 安装包（.dmg/.msi/.deb/.AppImage）走系统自身的签名机制，不要求 .sig。
    if !is_installer_kind(&kind) && signature.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "缺少签名文件内容（.sig 全文），没有签名客户端会拒绝安装".to_string(),
        ));
    }

    // 内容相同就不重复落盘：只改了 macOS 时，Windows/Linux 的包跟上一版逐字节相同。
    let sha = sha256_hex(&bytes);
    let stored_as = match find_stored_by_sha(&state.store, &sha)
        .await
        .map_err(ApiError::internal)?
    {
        Some(existing) => existing,
        None => {
            let dir = state.config.data_dir.join("releases").join(&id);
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(ApiError::internal)?;
            let fresh = format!("releases/{id}/{kind}-{}", sanitize(&filename));
            let path = state.config.data_dir.join(&fresh);
            tokio::fs::write(&path, &bytes)
                .await
                .map_err(ApiError::internal)?;
            fresh
        }
    };

    // 同类型重传视为替换，避免一个平台挂两个包。
    delete_artifact(&state.store, &id, kind)
        .await
        .map_err(ApiError::internal)?;
    let artifact_id = add_artifact(
        &state.store,
        &id,
        kind,
        &filename,
        &stored_as,
        bytes.len() as i64,
        &signature,
        &sha,
    )
    .await
    .map_err(ApiError::internal)?;

    Ok(Json(json!({ "ok": true, "id": artifact_id })))
}

pub async fn admin_delete_artifact(
    State(state): State<Shared>,
    Path((id, kind)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {

    delete_artifact(&state.store, &id, &kind)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct ReuseBody {
    /// 从哪个版本沿用产物（通常是上一版）。
    pub from: String,
}

/// `POST /api/admin/releases/:id/artifacts/reuse`
///
/// 把 `from` 版本的产物按**引用**复制过来，只补本版本还缺的 kind。
/// 不传文件、不占额外磁盘：同一个包在多个版本里共用一份。
pub async fn admin_reuse_artifacts(
    State(state): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ReuseBody>,
) -> ApiResult<Json<Value>> {

    if body.from == id {
        return Err(ApiError::BadRequest("不能从自己沿用产物".to_string()));
    }
    let src = artifacts_for(&state.store, &body.from)
        .await
        .map_err(ApiError::internal)?;
    if src.is_empty() {
        return Err(ApiError::BadRequest(
            "来源版本没有可沿用的产物".to_string(),
        ));
    }

    let copied = reuse_artifacts(&state.store, &id, &body.from)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true, "copied": copied })))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 回归：安装包和更新包曾经共用一套 kind，结果下载页把 Tauri 更新包
    // （tar.gz + .sig）当成安装包发给用户，下载下来装不上。
    // 回归：上传路由曾经没设 body 上限，axum 默认 2 MiB，真实安装包一传就
    // 「Error parsing multipart/form-data request」。上限必须明显大于桌面安装包体积。
    #[test]
    fn artifact_body_limit_fits_real_installers() {
        assert!(
            MAX_ARTIFACT_BYTES >= 200 * 1024 * 1024,
            "安装包常见 80–200 MB，上限不能低于 200 MiB"
        );
        assert!(
            MAX_ARTIFACT_BYTES > 2 * 1024 * 1024,
            "必须显式超过 axum 默认的 2 MiB，否则 multipart 会解析失败"
        );
    }

    #[test]
    fn installer_and_updater_kinds_are_disjoint() {
        let updaters = updater_kinds();
        for k in installer_kinds() {
            assert!(!updaters.contains(&k), "{k} 不能同时是安装包和更新包");
        }
        assert_eq!(updaters.len(), 3);
        // Windows 同时收 .msi 与 NSIS 的 .exe：老版本还在发 msi，新版本改出 nsis。
        assert_eq!(installer_kinds().len(), 5);
    }

    #[test]
    fn download_page_only_sees_installers() {
        for k in updater_kinds() {
            assert!(!is_installer_kind(k), "{k} 是更新包，不该出现在下载页");
        }
        for k in installer_kinds() {
            assert!(is_installer_kind(k));
        }
    }

    #[test]
    fn upload_accepts_both_sets() {
        let known = known_kinds();
        for k in updater_kinds() {
            assert!(known.contains(&k), "{k} 应可上传");
        }
        for k in installer_kinds() {
            assert!(known.contains(&k), "{k} 应可上传");
        }
        // 更新清单只认更新包：安装包没有 manifest platform key。
        for k in installer_kinds() {
            assert!(kind_platform_keys(k).is_empty(), "{k} 不该进更新清单");
        }
        assert_eq!(
            kind_platform_keys("macos_updater"),
            &["darwin-aarch64", "darwin-x86_64"]
        );
    }

    #[test]
    fn semver_compare_ignores_prerelease() {
        assert!(is_newer("1.2.4", "1.2.3"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(!is_newer("1.2.3", "1.2.4"));
        // 预发布后缀按官方口径忽略
        assert!(!is_newer("1.2.3-beta.1", "1.2.3"));
    }

    #[test]
    fn unparsable_current_counts_as_newer() {
        assert!(is_newer("1.0.0", "not-a-version"));
        assert!(!is_newer("bad", "1.0.0"));
    }

    #[test]
    fn manifest_maps_kinds_to_platform_keys() {
        let artifacts = vec![
            (
                "a1".to_string(),
                "macos_updater".to_string(),
                "app.tar.gz".to_string(),
                "SIG".to_string(),
            ),
            (
                "a2".to_string(),
                "windows_updater".to_string(),
                "app.msi".to_string(),
                "SIG2".to_string(),
            ),
        ];
        let m = build_manifest("1.0.0", "note", Some(1_752_300_000), "https://x", &artifacts)
            .expect("manifest");
        let platforms = m["platforms"].as_object().expect("platforms");
        assert!(platforms.contains_key("darwin-aarch64"));
        assert!(platforms.contains_key("darwin-x86_64"));
        assert!(platforms.contains_key("windows-x86_64"));
        assert!(!platforms.contains_key("linux-x86_64"));
        assert_eq!(
            platforms["darwin-aarch64"]["url"],
            "https://x/api/downloads/files/a1"
        );
        assert_eq!(platforms["darwin-aarch64"]["signature"], "SIG");
    }

    #[test]
    fn manifest_is_none_without_signed_artifact() {
        let artifacts = vec![(
            "a1".to_string(),
            "macos_updater".to_string(),
            "app.tar.gz".to_string(),
            "   ".to_string(),
        )];
        assert!(build_manifest("1.0.0", "", None, "https://x", &artifacts).is_none());
    }

    #[tokio::test]
    async fn reuse_copies_references_without_new_files() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let tenant = crate::schema::DEFAULT_TENANT;

        let v1 = create_release(&store, tenant, "1.0.0", "").await.expect("v1");
        add_artifact(&store, &v1.id, "macos_dmg", "P.dmg", "releases/x/a", 10, "", "sha-a")
            .await
            .expect("a1");
        add_artifact(&store, &v1.id, "windows_msi", "P.msi", "releases/x/b", 20, "", "sha-b")
            .await
            .expect("a2");

        let v2 = create_release(&store, tenant, "1.1.0", "").await.expect("v2");
        // v2 已经有 macOS 包，沿用时应保留自己的，不覆盖。
        add_artifact(&store, &v2.id, "macos_dmg", "P2.dmg", "releases/y/c", 11, "", "sha-c")
            .await
            .expect("a3");

        let copied = reuse_artifacts(&store, &v2.id, &v1.id).await.expect("reuse");
        assert_eq!(copied, vec!["windows_msi".to_string()]);

        let got = artifacts_for(&store, &v2.id).await.expect("list");
        assert_eq!(got.len(), 2);
        let mac = got.iter().find(|a| a.kind == "macos_dmg").expect("mac");
        assert_eq!(mac.filename, "P2.dmg", "已有产物不应被覆盖");
        let win = got.iter().find(|a| a.kind == "windows_msi").expect("win");
        // 复用的是同一条磁盘文件，不是复制出来的新文件。
        assert_eq!(win.stored_as, "releases/x/b");
        assert_eq!(win.sha256, "sha-b");
    }

    #[tokio::test]
    async fn same_content_reuses_one_stored_file() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let tenant = crate::schema::DEFAULT_TENANT;
        let v1 = create_release(&store, tenant, "1.0.0", "").await.expect("v1");
        add_artifact(&store, &v1.id, "macos_dmg", "P.dmg", "releases/x/a", 10, "", "sha-a")
            .await
            .expect("a");

        assert_eq!(
            find_stored_by_sha(&store, "sha-a").await.expect("find"),
            Some("releases/x/a".to_string())
        );
        // 没存过的内容返回 None，走正常落盘。
        assert_eq!(find_stored_by_sha(&store, "sha-z").await.expect("find"), None);
        // 空哈希不参与去重（历史数据没有 sha）。
        assert_eq!(find_stored_by_sha(&store, "").await.expect("find"), None);
    }

    #[test]
    fn timestamp_renders_rfc3339_utc() {
        // 2026-07-12T04:00:00Z
        assert_eq!(
            time_like_iso(1_783_828_800).unwrap_or_default(),
            "2026-07-12T04:00:00Z"
        );
    }

    #[test]
    fn unknown_kind_has_no_platform_keys() {
        assert!(kind_platform_keys("nope").is_empty());
    }

    /// 回归：这 6 个管理端端点曾经**完全没有鉴权**——未登录即可列出、新建、
    /// 删除、发布安装包，也能上传任意文件。
    #[tokio::test]
    async fn admin_endpoints_reject_anonymous() {
        use axum::body::Body;
        use axum::http::{Method, Request, StatusCode};
        use tower::ServiceExt;

        let store = AdminStore::connect_in_memory().await.expect("store");
        let state = crate::state::AppState::new(store, crate::config::AdminConfig::default());
        let app = crate::http::router(state.clone()).with_state(state);

        let json = "application/json";
        let multipart = "multipart/form-data; boundary=x";
        let cases: Vec<(Method, &str, Option<(&str, &str)>)> = vec![
            (Method::GET, "/api/admin/releases", None),
            (
                Method::POST,
                "/api/admin/releases",
                Some((json, r#"{"version":"9.9.9"}"#)),
            ),
            (Method::DELETE, "/api/admin/releases/r1", None),
            (
                Method::POST,
                "/api/admin/releases/r1/publish",
                Some((json, r#"{"published":false}"#)),
            ),
            (
                Method::POST,
                "/api/admin/releases/r1/artifacts",
                Some((multipart, "")),
            ),
            (
                Method::DELETE,
                "/api/admin/releases/r1/artifacts/macos_updater",
                None,
            ),
        ];

        for (method, uri, body) in cases {
            let mut req = Request::builder().method(method.clone()).uri(uri);
            let payload = match body {
                Some((content_type, raw)) => {
                    req = req.header("content-type", content_type);
                    Body::from(raw)
                }
                None => Body::empty(),
            };
            let resp = app
                .clone()
                .oneshot(req.body(payload).expect("request"))
                .await
                .expect("response");
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
    }

    /// 下载页清单：只列已发布版本，并给出可直接下载的 URL（不要求签名）。
    #[tokio::test]
    async fn downloads_list_only_returns_published() {
        let store = AdminStore::connect_in_memory().await.expect("store");
        let tenant = crate::schema::DEFAULT_TENANT;

        let released = create_release(&store, tenant, "1.0.0", "notes")
            .await
            .expect("release");
        // 同一版本里同时放更新包和安装包：下载页只应看到安装包。
        add_artifact(
            &store,
            &released.id,
            "macos_updater",
            "app.tar.gz",
            "releases/x/a",
            1024,
            "",
            "",
        )
        .await
        .expect("updater");
        add_artifact(
            &store,
            &released.id,
            "macos_dmg",
            "Pointer_1.0.0_universal.dmg",
            "releases/x/b",
            2048,
            "",
            "abc123",
        )
        .await
        .expect("installer");
        set_published(&store, tenant, &released.id, true)
            .await
            .expect("publish");
        // 草稿版本不应出现
        create_release(&store, tenant, "1.1.0", "").await.expect("draft");

        let state = crate::state::AppState::new(store, crate::config::AdminConfig::default());
        let Json(v) = downloads_list(
            axum::extract::State(state),
            HeaderMap::new(),
            Query(DownloadsQuery { limit: None }),
        )
        .await
        .expect("list");

        let items = v["items"].as_array().expect("items");
        assert_eq!(items.len(), 1, "未发布的版本不应出现在下载页");
        assert_eq!(items[0]["version"], "1.0.0");
        let artifacts = items[0]["artifacts"].as_array().expect("artifacts");
        assert_eq!(artifacts.len(), 1, "更新包不该出现在下载页；安装包未签名也要列出");
        assert_eq!(artifacts[0]["kind"], "macos_dmg");
        assert_eq!(artifacts[0]["filename"], "Pointer_1.0.0_universal.dmg");
        assert!(artifacts[0]["downloadUrl"]
            .as_str()
            .expect("url")
            .contains("/api/downloads/files/"));
    }
}
