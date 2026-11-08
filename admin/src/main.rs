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
use pointer_admin::seed;
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

/// 桌面端登录成功后浏览器会被送到 web 根（企业部署下 web 根就是控制面）。
/// 直接把员工丢进管理端登录页是错的：他不是管理员，也不该再登一次。
const DESKTOP_OAUTH_DONE: &str = r##"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>登录成功</title>
<style>
:root{--bg:#FAFAFA;--fg:#18181B;--muted:#71717A;--line:#E5E5E8;--accent:#0071EB}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;
background:var(--bg);color:var(--fg);
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}
.card{width:380px;padding:32px;background:#fff;border:1px solid var(--line);border-radius:12px;text-align:center}
.mark{width:44px;height:44px;border-radius:50%;background:#E7F3EC;color:#1F7A4D;
display:flex;align-items:center;justify-content:center;margin:0 auto 14px;font-size:22px}
h1{margin:0 0 8px;font-size:17px}
p{margin:0;color:var(--muted);font-size:13px;line-height:1.7}
</style></head><body><div class="card">
<div class="mark">✓</div>
<h1>登录成功</h1>
<p>可以关闭这个页面，回到 Pointer 客户端继续使用。</p>
</div></body></html>"##;

async fn index(State(state): State<Shared>, uri: axum::http::Uri) -> Response {
    let desktop_done = uri
        .query()
        .map(|q| q.split('&').any(|kv| kv == "desktop_oauth=success"))
        .unwrap_or(false);
    if desktop_done {
        return Html(DESKTOP_OAUTH_DONE).into_response();
    }
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
    // 密钥模式决定密文能不能跨机器解，部署时最容易搞错，启动就说清楚。
    if std::env::var(pointer_admin::crypto::MASTER_KEY_ENV)
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        log::info!("企业密钥加密：使用 {}（可跨实例解密）", pointer_admin::crypto::MASTER_KEY_ENV);
    } else {
        log::warn!(
            "企业密钥加密：未配置 {}，按本机标识派生——换机/多实例部署后旧密钥解不开",
            pointer_admin::crypto::MASTER_KEY_ENV
        );
    }

    let store = AdminStore::connect(&db_url)
        .await
        .context("连接控制面数据库失败")?;
    bootstrap(&store, &config).await?;

    // `pointer-admin seed` —— 装一遍内置服务商（qwen / deepseek / 豆包）与默认档位。
    // 幂等：已存在的 slug 跳过，不覆盖管理员改过的配置。
    if std::env::args().any(|a| a == "seed") {
        let report = seed::seed(&store, pointer_admin::schema::DEFAULT_TENANT).await?;
        println!(
            "服务商：新建 {} 个，跳过 {} 个；模型新建 {} 个；档位映射：{}",
            report.providers_created,
            report.providers_skipped,
            report.models_created,
            if report.tier_created { "已建" } else { "已存在" }
        );
        return Ok(());
    }

    // 全新库：把内置的三家服务商（百炼 / DeepSeek / 豆包）和默认档位种上，
    // 否则管理员登录后是一片空白。库里已经有服务商就不动 —— 管理员删掉的
    // 不该在重启后又冒出来。
    if pointer_admin::resources::list(&store, &config.tenant_id, Some(pointer_admin::resources::KIND_PROVIDER))
        .await?
        .is_empty()
    {
        match seed::seed(&store, &config.tenant_id).await {
            Ok(r) => log::info!(
                "初始化服务商：新建 {} 个，模型 {} 个",
                r.providers_created,
                r.models_created
            ),
            Err(e) => log::warn!("初始化服务商失败（不影响启动）：{e}"),
        }
    }

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
    if !config.captcha_required {
        log::warn!(
            "登录验证码已关闭（POINTER_ADMIN_CAPTCHA=off）——只应在自动化测试或内网联调时这么配"
        );
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("监听 {addr} 失败"))?;

    log::info!("控制面已启动：http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
