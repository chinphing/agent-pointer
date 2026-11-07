//! Pointer 企业控制面进程。
//!
//! 启动顺序：读配置 → 连库迁移 → 种子数据（首个管理员 / OAuth 客户端）→ 挂路由 → 监听。
//! 单独一个进程，server 与桌面客户端都是它的 OAuth 客户端。

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::json;

use pointer_admin::config::AdminConfig;
use pointer_admin::db::AdminStore;
use pointer_admin::oauth;
use pointer_admin::rbac;
use pointer_admin::state::{AppState, Shared};
use pointer_admin::users;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Pointer 企业控制面</title>
<style>
body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;
background:#f6f7f9;color:#1f2328;
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
.box{max-width:520px;padding:36px;background:#fff;border:1px solid #d9dde3;border-radius:12px}
h1{margin:0 0 12px;font-size:20px}
p{margin:0 0 10px;color:#57606a;font-size:14px;line-height:1.7}
code{background:#f0f2f5;padding:2px 6px;border-radius:4px;font-size:13px}
</style></head><body><div class="box">
<h1>控制面已启动</h1>
<p>前端工程还没有构建，所以这里显示的是占位页。接口本身已经可用。</p>
<p>构建前端：在 <code>admin/web</code> 下执行 <code>npm install &amp;&amp; npm run build</code>，
产物落到 <code>admin/web/dist</code>，重启本进程即可自动挂载。</p>
<p>也可以直接用接口：<code>POST /api/admin/auth/login</code>。</p>
</div></body></html>"#;

async fn index(State(state): State<Shared>) -> Response {
    if let Some(dir) = &state.config.static_dir {
        let path = dir.join("index.html");
        if let Ok(html) = tokio::fs::read_to_string(&path).await {
            return Html(html).into_response();
        }
    }
    Html(PLACEHOLDER).into_response()
}

async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({ "error": "not_found", "message": "接口不存在" })),
    )
        .into_response()
}

async fn bootstrap(store: &AdminStore, cfg: &AdminConfig) -> Result<()> {
    let existing = users::list(store, &cfg.tenant_id).await?;
    if existing.is_empty() {
        match (
            cfg.bootstrap_user.as_deref(),
            cfg.bootstrap_password.as_deref(),
        ) {
            (Some(username), Some(password)) => {
                let user = users::create(store, &cfg.tenant_id, username, username, password)
                    .await
                    .context("创建首个管理员失败")?;
                rbac::assign_role(store, &cfg.tenant_id, &user.id, rbac::SUPER_ADMIN)
                    .await
                    .context("授予 super_admin 失败")?;
                log::info!("已创建首个管理员账号：{username}");
            }
            _ => log::warn!(
                "库里还没有任何账号。请设置 POINTER_ADMIN_BOOTSTRAP_USER 与 \
                 POINTER_ADMIN_BOOTSTRAP_PASSWORD 后重启，否则没人能登录。"
            ),
        }
    }

    match cfg.oauth_secret.as_deref() {
        Some(secret) => {
            oauth::clients::upsert(
                store,
                oauth::clients::CLIENT_SERVER,
                secret,
                oauth::clients::KIND_SERVER,
                &[cfg.server_redirect_uri.as_str()],
            )
            .await?;
            oauth::clients::upsert(
                store,
                oauth::clients::CLIENT_DESKTOP,
                secret,
                oauth::clients::KIND_DESKTOP,
                &[],
            )
            .await?;
        }
        None => log::warn!(
            "未配置 POINTER_ADMIN_OAUTH_SECRET：/oauth/* 与 /auth/app/token 会返回 503"
        ),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = AdminConfig::from_env();
    let db_url = config.db_url();
    log::info!("控制面数据库：{db_url}");

    let store = AdminStore::connect(&db_url)
        .await
        .context("连接控制面数据库失败")?;
    bootstrap(&store, &config).await?;

    let state = AppState::new(store, config.clone());
    let mut app = Router::new()
        .merge(oauth::router())
        .merge(pointer_admin::http::router())
        .route("/", get(index))
        .route("/admin", get(index))
        .route("/admin/*rest", get(index))
        .fallback(not_found);

    if let Some(dir) = &config.static_dir {
        log::info!("静态资源目录：{}", dir.display());
        app = app.nest_service(
            "/assets",
            tower_http::services::ServeDir::new(dir.join("assets")),
        );
    }

    let app = app.with_state(state);

    let addr: SocketAddr = config
        .addr
        .parse()
        .with_context(|| format!("地址格式不正确：{}", config.addr))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("监听 {addr} 失败"))?;

    log::info!("控制面已启动：http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
