//! 私有部署的首页：官网营销落地页的移植版。
//!
//! 对照 `pointer-official/apps/web/src/app/page.tsx` 与 `components/VideoDemos.tsx`：
//! 主视觉 + 三个演示视频 + 法律声明。文案、按钮、视频清单与官网一致。
//!
//! 由控制面直接返回 HTML，不依赖前端构建——营销页不需要管理端的 Vue 外壳。

use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::http::auth::{current_auth, AdminAuth};
use crate::state::Shared;

/// 主视觉背景，取自官网 `public/hero-bg.png`。
const HERO_BG: &[u8] = include_bytes!("../assets/hero-bg.png");

/// 应用图标，取自客户端 `src-tauri/icons/icon.png`（512×512）。
const APP_ICON: &[u8] = include_bytes!("../assets/app-icon.png");

pub fn router() -> Router<Shared> {
    Router::new()
        .route("/", get(index))
        .route("/hero-bg.png", get(hero_bg))
        .route("/app-icon.png", get(app_icon))
}

/// `/` 返回营销页。
///
/// **内容不按登录态分流，只有导航上那一个入口变**：登录了给「进入控制台」，
/// 没登录给「登录」。不做重定向——同一个 URL 返回两种页面会让「刷新看到营销页、
/// 点导航看到控制台」这种错乱重现。
///
/// 登录态从会话 cookie 读，读不到就当未登录：首页不能因为会话服务抖动而 500。
async fn index(State(state): State<Shared>, headers: HeaderMap) -> Response {
    let auth = current_auth(&state, &headers).await.ok();
    Html(render(auth.as_ref())).into_response()
}

/// 把登录态拼进首页。
fn render(auth: Option<&AdminAuth>) -> String {
    let (nav, avatar, hero) = match auth {
        Some(a) => {
            // 管理员也是普通用户：个人中心永远有，控制面板按权限追加。
            let mut nav = String::from(r#"<a href="/me">个人中心</a>"#);
            if has_console_access(a) {
                nav.push_str(r#"<a href="/admin/overview">控制面板</a>"#);
            }
            (
                nav,
                format!(
                    r#"<a class="avatar" href="/me" title="个人中心">{}</a>"#,
                    avatar_initial(&a.user.display_name, &a.user.username)
                ),
                r#"<a class="btn" href="/me">个人中心</a>"#.to_string(),
            )
        }
        None => (
            r#"<a href="/login">登录</a>"#.to_string(),
            String::new(),
            r#"<a class="btn" href="/login">开始使用</a>"#.to_string(),
        ),
    };
    HOMEPAGE
        .replace("<!--NAV_SESSION-->", &nav)
        .replace("<!--AVATAR-->", &avatar)
        .replace("<!--HERO_SESSION-->", &hero)
}

/// 能进控制面板的权限，与 `admin/web/src/routes.ts` 里 console 段各页的 `perm` 对应。
///
/// 有任意一条就显示「控制面板」入口。没有就不显示——给一个点进去会被守卫弹回的
/// 链接，比不给更糟。
const CONSOLE_PERMS: &[&str] = &[
    "system:read",
    "user:read",
    "role:read",
    "provider:read",
    "release:read",
    "usage:read",
    "audit:read",
];

fn has_console_access(auth: &AdminAuth) -> bool {
    CONSOLE_PERMS.iter().any(|p| auth.has(p))
}

/// 头像里的字：取显示名首字，没有就用账号首字母——和控制面的规则一致。
fn avatar_initial(display_name: &str, username: &str) -> String {
    let name = if display_name.trim().is_empty() {
        username
    } else {
        display_name
    };
    let first = name.trim().chars().next().unwrap_or('?');
    escape_html(&first.to_uppercase().to_string())
}

/// 用户名是用户自己填的，直接拼进 HTML 就是 XSS。
fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

async fn hero_bg() -> Response {
    png(HERO_BG)
}

async fn app_icon() -> Response {
    png(APP_ICON)
}

fn png(bytes: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

const HOMEPAGE: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Pointer · 计算机智能体</title>
<link rel="icon" type="image/png" href="/app-icon.png">
<style>
*{box-sizing:border-box}
body{margin:0;background:#fff;color:#18181B;
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
-webkit-font-smoothing:antialiased}
a{color:inherit;text-decoration:none}

/* 顶部导航 */
header{position:relative;z-index:20;height:56px;display:flex;align-items:center;
padding:0 20px;border-bottom:1px solid rgba(0,0,0,.06);
background:rgba(255,255,255,.92);backdrop-filter:blur(8px)}
.brand{display:flex;align-items:center;gap:9px;font-weight:600;font-size:15px}
.brand img{width:24px;height:24px;border-radius:6px;display:block}
nav{display:flex;align-items:center;gap:22px;font-size:14px;color:rgba(24,24,27,.72);
margin-left:auto}
nav a:hover{color:#18181B}
/* 与控制面右上角的用户入口一致：28px 圆形头像 + 文案 */
.avatar{width:28px;height:28px;flex:0 0 28px;border-radius:50%;margin-left:22px;
background:#e8e8eb;color:#52525b;font-size:11px;font-weight:600;
display:flex;align-items:center;justify-content:center}
.avatar:hover{background:#d4d4d8}

/* 主视觉 */
.hero{position:relative;overflow:hidden;border-bottom:1px solid rgba(0,0,0,.07);
min-height:min(480px,60vh);display:flex;flex-direction:column}
.hero-bg{position:absolute;inset:0;background:url(/hero-bg.png) 50% 10%/cover no-repeat;
filter:blur(2px) brightness(1.32) saturate(1.12)}
.hero-shade{position:absolute;inset:0;
background:
radial-gradient(ellipse 105% 92% at 50% 22%,transparent 55%,rgba(15,23,42,.08) 100%),
linear-gradient(90deg,rgba(2,6,23,.4),rgba(2,6,23,.1) 45%,transparent),
linear-gradient(180deg,rgba(255,255,255,.08),rgba(255,255,255,.04) 50%,rgba(2,6,23,.03))}
.hero-inner{position:relative;z-index:10;flex:1;display:flex;flex-direction:column;
justify-content:center;max-width:1024px;margin:0 auto;width:100%;padding:44px 16px}
.eyebrow{margin:0 0 32px;font-size:14px;font-weight:500;letter-spacing:.14em;
color:#CFFAFE;text-shadow:0 1px 12px rgba(0,0,0,.45)}
h1{margin:0 0 20px;font-size:2.5rem;font-weight:600;line-height:1.12;letter-spacing:-.02em;
color:#fff;text-shadow:0 1px 3px rgba(0,0,0,.45),0 6px 56px rgba(0,0,0,.55)}
.lead{margin:0 0 28px;max-width:640px;font-size:1.0625rem;line-height:1.75;color:#fff;
text-shadow:0 1px 2px rgba(0,0,0,.35),0 4px 28px rgba(0,0,0,.45)}
.actions{display:flex;flex-wrap:wrap;gap:12px}
.btn{display:inline-flex;align-items:center;justify-content:center;padding:10px 24px;
border-radius:999px;font-size:14px;font-weight:500;border:1px solid rgba(255,255,255,.3);
background:rgba(255,255,255,.08);color:#fff;backdrop-filter:blur(2px);transition:.15s}
.btn:hover{background:rgba(255,255,255,.12);border-color:rgba(255,255,255,.45)}
.btn.primary{background:#fff;color:#0F172A;font-weight:600;border-color:#fff;
box-shadow:0 4px 24px rgba(0,0,0,.2)}
.btn.primary:hover{background:rgba(255,255,255,.95)}

/* 正文 */
main{max-width:1024px;margin:0 auto;padding:48px 16px 80px}
.sec-head{display:flex;align-items:center;gap:12px;margin-bottom:8px}
.rule{height:1px;width:40px;background:linear-gradient(90deg,#0B6CF0,rgba(11,108,240,.3))}
h2{margin:0;font-size:1.75rem;font-weight:600;letter-spacing:-.02em}
.sec-sub{margin:8px 0 32px;padding-left:52px;max-width:640px;font-size:14px;
line-height:1.7;color:rgba(0,0,0,.5)}

/* 演示视频 */
.demos{display:grid;gap:24px;grid-template-columns:repeat(auto-fit,minmax(280px,1fr))}
.card{overflow:hidden;border-radius:16px;border:1px solid rgba(0,0,0,.05);
background:#fff;box-shadow:0 1px 3px rgba(0,0,0,.06)}
.frame{position:relative;aspect-ratio:16/9;background:#000}
.frame video{width:100%;height:100%;object-fit:contain;display:block}
.cover{position:absolute;inset:0;width:100%;height:100%;padding:0;border:0;
background:#000;cursor:pointer;overflow:hidden}
.cover video{width:100%;height:100%;object-fit:cover;opacity:.95;pointer-events:none}
.cover:hover video{opacity:1}
.cover span{position:absolute;left:0;right:0;bottom:0;padding:10px 12px;text-align:left;
font-size:13px;color:#fff;background:linear-gradient(transparent,rgba(0,0,0,.65))}
.card h3{margin:0;padding:12px 14px;font-size:14px;font-weight:500}

/* 法律声明 */
.legal{margin-top:88px;padding:20px;border-radius:16px;border:1px solid rgba(0,0,0,.05);
background:rgba(255,255,255,.7);font-size:14px;line-height:1.8;color:rgba(0,0,0,.65);
box-shadow:0 1px 2px rgba(0,0,0,.04)}
.legal a{color:#0B6CF0;text-decoration:underline;text-underline-offset:4px}
</style>
</head>
<body>
<header>
  <a class="brand" href="/"><img src="/app-icon.png" alt="Pointer" width="24" height="24">Pointer</a>
  <nav>
    <a href="/download">下载</a>
    <!--NAV_SESSION-->
  </nav>
  <!--AVATAR-->
</header>

<div class="hero">
  <div class="hero-bg"></div>
  <div class="hero-shade"></div>
  <div class="hero-inner">
    <p class="eyebrow">类人操作 · 视觉感知 · 跨应用</p>
    <h1>像人一样看屏幕、动鼠标的计算机智能体</h1>
    <p class="lead">
      面向个人与企业，作为您的个人助理或企业员工，跟人类操作一样接管你的电脑，
      实现跨应用的自动化工作流。从此，标准作业流程（SOP）的自动化，
      不再是产品研发团队的专属技能，任何人都可以。
    </p>
    <div class="actions">
      <a class="btn primary" href="/download">下载客户端</a>
      <!--HERO_SESSION-->
      <a class="btn" href="https://github.com/chinphing/agent-pointer" target="_blank" rel="noreferrer">GitHub</a>
    </div>
  </div>
</div>

<main>
  <div class="sec-head"><span class="rule"></span><h2>演示视频</h2></div>
  <p class="sec-sub">在真实桌面场景中的能力展示，点击封面即可在当前页内播放。</p>

  <div class="demos" id="demos">
    <article class="card">
      <div class="frame">
        <button class="cover" type="button" data-src="https://oss.readflowai.com/video/%E5%BE%AE%E4%BF%A1%E5%B0%8F%E6%B8%B8%E6%88%8F%E2%80%94%E6%95%B0%E7%8B%AC.mp4">
          <video src="https://oss.readflowai.com/video/%E5%BE%AE%E4%BF%A1%E5%B0%8F%E6%B8%B8%E6%88%8F%E2%80%94%E6%95%B0%E7%8B%AC.mp4#t=2" preload="metadata" muted playsinline></video>
          <span>▶ 点击播放</span>
        </button>
      </div>
      <h3>玩微信小游戏—数独</h3>
    </article>
    <article class="card">
      <div class="frame">
        <button class="cover" type="button" data-src="https://oss.readflowai.com/video/%E5%BE%AE%E4%BF%A1%E8%81%8A%E5%A4%A9%E8%AE%B0%E5%BD%95.mp4">
          <video src="https://oss.readflowai.com/video/%E5%BE%AE%E4%BF%A1%E8%81%8A%E5%A4%A9%E8%AE%B0%E5%BD%95.mp4#t=2" preload="metadata" muted playsinline></video>
          <span>▶ 点击播放</span>
        </button>
      </div>
      <h3>用最笨但合规的方法查微信聊天记录</h3>
    </article>
    <article class="card">
      <div class="frame">
        <button class="cover" type="button" data-src="https://oss.readflowai.com/video/%E7%A8%8E%E5%8A%A1%E5%B1%80%E7%BC%B4%E7%A8%8E.mp4">
          <video src="https://oss.readflowai.com/video/%E7%A8%8E%E5%8A%A1%E5%B1%80%E7%BC%B4%E7%A8%8E.mp4#t=2" preload="metadata" muted playsinline></video>
          <span>▶ 点击播放</span>
        </button>
      </div>
      <h3>操作手机镜像税务局缴税</h3>
    </article>
  </div>

  <section class="legal">
    <p>
      声明：本站提供计算机智能体与自动化能力，相关功能仅限在合法、合规且已获授权的场景中使用。
      涉及验证码、人机校验等能力时，不得用于绕过第三方平台安全机制或违反目标平台规则的用途。
      详情请见 <a href="/legal">《法律声明》</a>。
    </p>
  </section>
</main>

<script>
// 点击封面就地播放：和官网一样，不跳页、不弹层。
document.getElementById('demos').addEventListener('click', function (e) {
  var btn = e.target.closest('.cover');
  if (!btn) return;
  var frame = btn.parentNode;
  var src = btn.getAttribute('data-src');
  var v = document.createElement('video');
  v.src = src;
  v.controls = true;
  v.autoplay = true;
  v.playsInline = true;
  v.muted = true;
  frame.innerHTML = '';
  frame.appendChild(v);
});
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn auth_with(name: &str) -> AdminAuth {
        auth_with_perms(name, &[])
    }

    fn auth_with_perms(name: &str, perms: &[&str]) -> AdminAuth {
        AdminAuth {
            user: crate::users::User {
                id: "u1".into(),
                tenant_id: "default".into(),
                username: "u1".into(),
                display_name: name.into(),
                status: "active".into(),
                created_at: 0,
                last_login_at: None,
                phone: None,
            },
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            roles: Vec::new(),
        }
    }

    #[test]
    fn logged_out_shows_login_entry() {
        let html = render(None);
        assert!(html.contains(r#"<a href="/login">登录</a>"#));
        assert!(html.contains("开始使用"));
        assert!(!html.contains("个人中心"));
        assert!(!html.contains("控制面板"));
        assert!(!html.contains(r#"class="avatar""#));
    }

    #[test]
    fn member_sees_personal_center_only() {
        let html = render(Some(&auth_with("张三")));
        assert!(html.contains(r#"<a href="/me">个人中心</a>"#));
        // 成员没有控制台权限，不该看到一个点进去会被弹回的入口
        assert!(!html.contains("控制面板"));
        assert!(html.contains(r#"<a class="avatar" href="/me""#));
        // 只露头像首字，不把完整用户名摆在导航里
        assert!(html.contains(">张</a>"));
        assert!(!html.contains("张三"));
    }

    #[test]
    fn admin_sees_both_entries() {
        let html = render(Some(&auth_with_perms("李四", &["system:read"])));
        assert!(html.contains(r#"<a href="/me">个人中心</a>"#));
        assert!(html.contains(r#"<a href="/admin/overview">控制面板</a>"#));
    }

    #[test]
    fn console_entry_needs_a_console_permission() {
        // 只有支付权限不算控制台入口：权限判断要按控制台自己的权限集来
        let html = render(Some(&auth_with_perms("王五", &["payment:read"])));
        assert!(!html.contains("控制面板"));
    }

    #[test]
    fn display_name_is_escaped() {
        // 头像只取首字，但首字同样要转义：显示名可以是 `<` 开头
        let html = render(Some(&auth_with("<img src=x onerror=alert(1)>")));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains(">&lt;</a>"));
    }

    #[test]
    fn avatar_falls_back_to_username() {
        assert_eq!(avatar_initial("", "alice"), "A");
        assert_eq!(avatar_initial("  ", "bob"), "B");
        assert_eq!(avatar_initial("张三", "zhangsan"), "张");
    }
}
