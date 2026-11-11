//! 应用装配：路由、静态资源、SPA 回落。
//!
//! 从 `main.rs` 抽出来，是为了让私有仓能复用同一套装配——
//! 私有二进制只做三件事：装 [`crate::ext`] 实现、调 [`build_app`]、监听。

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::json;

use crate::state::Shared;

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

/// 非 API 路径回落到 SPA 的 index（history 路由刷新不 404）。
///
/// `/api`、`/auth`、`/oauth` 仍然返回 JSON 404 —— 否则打错的接口会被吞成首页，
/// 排查时看到的是一张 HTML 而不是「接口不存在」。
async fn spa_or_404(State(state): State<Shared>, uri: axum::http::Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api") || path.starts_with("/auth") || path.starts_with("/oauth") {
        return not_found().await;
    }
    index(State(state), uri).await
}

/// 装配整棵路由：OAuth、管理端 API、私有扩展的公开路由、更新与下载、
/// SPA 回落、静态资源。
///
/// 私有仓在自己的 `main` 里先 `ext::install(...)` 再调这里，装配结果就带上
/// 了私有路由；不装则与开源版完全一致。
pub fn build_app(state: Shared) -> Router {
    let mut app = Router::new()
        .merge(crate::oauth::router())
        .merge(crate::http::router(state.clone()))
        // 营销首页与法律声明：由控制面直接返回 HTML，不依赖前端构建。
        // 首页按登录态换导航入口，所以放在这里而不是静态目录。
        .merge(crate::home::router())
        .merge(crate::legal::router())
        // 私有仓的免鉴权路由（支付回调、短信发码）；开源版为空 Router。
        .merge(crate::ext::public_router())
        .route("/api/updates/latest", get(crate::releases::updates_latest))
        .route(
            "/api/downloads/releases",
            get(crate::releases::downloads_list),
        )
        .route(
            "/api/downloads/files/:id",
            get(crate::releases::download_file),
        )
        // history 路由：非 API 路径全部交给 SPA，否则刷新 `/me/billing` 会 404。
        .fallback(spa_or_404);

    if let Some(dir) = &state.config.static_dir {
        app = app.nest_service(
            "/assets",
            tower_http::services::ServeDir::new(dir.join("assets")),
        );
    }

    app.with_state(state)
}
