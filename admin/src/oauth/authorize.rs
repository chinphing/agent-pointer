//! `GET/POST /oauth/authorize`：员工登录页 + 授权码签发。
//!
//! 页面是内联 HTML（不依赖前端工程），所以控制面没构建前端时整条登录链路也能跑通。

use axum::extract::{Form, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::http::error::{ApiError, ApiResult};
use crate::oauth::clients;
use crate::state::Shared;
use crate::users;

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub code_challenge: String,
    #[serde(default)]
    pub code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeForm {
    pub username: String,
    pub password: String,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub code_challenge: String,
    #[serde(default)]
    pub captcha_id: String,
    #[serde(default)]
    pub captcha: String,
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn urlenc(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

async fn validate(state: &Shared, q: &AuthorizeQuery) -> ApiResult<()> {
    if state.config.oauth_secret.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "控制面尚未配置 OAuth 客户端（POINTER_ADMIN_OAUTH_SECRET）".to_string(),
        ));
    }
    if q.code_challenge.trim().is_empty() {
        return Err(ApiError::BadRequest("缺少 code_challenge（必须使用 PKCE）".to_string()));
    }
    if let Some(method) = q.code_challenge_method.as_deref() {
        if !method.eq_ignore_ascii_case("S256") {
            return Err(ApiError::BadRequest(
                "仅支持 code_challenge_method=S256".to_string(),
            ));
        }
    }
    if !clients::exists(&state.store, &q.client_id).await? {
        return Err(ApiError::BadRequest(format!(
            "未知的 client_id：{}",
            q.client_id
        )));
    }
    if !clients::redirect_allowed(&state.store, &q.client_id, &q.redirect_uri).await? {
        return Err(ApiError::BadRequest(
            "redirect_uri 不在白名单内".to_string(),
        ));
    }
    Ok(())
}

pub async fn page(
    State(state): State<Shared>,
    headers: axum::http::HeaderMap,
    Query(q): Query<AuthorizeQuery>,
) -> ApiResult<Response> {
    validate(&state, &q).await?;
    // 控制面已登录（网页端登录过、或私有仓的短信登录过的同一个 Cookie）就不再
    // 问一遍密码：直接发码跳回客户端。以前这里无条件渲染表单，于是「网页里明明
    // 登录着，桌面点登录还要再输一次账号密码」。
    if let Some(resp) = session_redirect(&state, &headers, &q).await {
        return Ok(resp);
    }
    let html = render_page(&state, &q, None).await?;
    Ok(Html(html).into_response())
}

/// 已登录 → 发码并跳回 `redirect_uri`；没有有效会话返回 `None`（调用方渲染表单）。
///
/// 任何一步失败都退回表单而不是 500：员工看到的应该是登录框，不是错误页。
async fn session_redirect(
    state: &Shared,
    headers: &axum::http::HeaderMap,
    q: &AuthorizeQuery,
) -> Option<Response> {
    let auth = crate::http::auth::current_auth(state, headers).await.ok()?;
    let code = crate::oauth::code::issue(
        &state.store,
        &q.client_id,
        &auth.user.id,
        &q.redirect_uri,
        &q.code_challenge,
    )
    .await
    .ok()?;
    Some(
        Redirect::to(&redirect_target(
            &q.redirect_uri,
            &code,
            q.state.as_deref(),
        ))
        .into_response(),
    )
}

/// `redirect_uri?code=…[&state=…]`，参数按 RFC 3986 编码。
fn redirect_target(redirect_uri: &str, code: &str, state: Option<&str>) -> String {
    let mut target = format!("{}?code={}", redirect_uri, urlenc(code));
    if let Some(st) = state.filter(|s| !s.is_empty()) {
        target.push_str(&format!("&state={}", urlenc(st)));
    }
    target
}

/// 渲染登录页。失败后要能带着错误信息重新渲染——所以验证码也是这里发的。
async fn render_page(
    state: &Shared,
    q: &AuthorizeQuery,
    error: Option<&str>,
) -> ApiResult<String> {
    let (captcha_id, captcha_image) = crate::captcha::issue(&state.store)
        .await
        .map_err(ApiError::internal)?;
    let error_html = match error {
        Some(msg) => format!(
            r#"<p class="err">{}</p>"#,
            escape_html(msg)
        ),
        None => String::new(),
    };
    // 登录方式：扩展提供的若干种（短信、扫码、SSO…）+ 内置账号密码。
    // 和网页端登录页同一套形态 —— 标签切换，同一时刻只显示一种；没有扩展时
    // 只剩账号密码，页面与开源版完全一致。
    let methods = crate::ext::login_methods();
    let has_ext = !methods.is_empty();
    let mut tabs_html = String::new();
    let mut panes_html = String::new();
    if has_ext {
        tabs_html.push_str("<div class=\"tabs\">");
        for (i, m) in methods.iter().enumerate() {
            let active = if i == 0 { " active" } else { "" };
            tabs_html.push_str(&format!(
                r#"<button type="button" class="tab{active}" data-tab="{key}">{label}</button>"#,
                key = escape_html(&m.key),
                label = escape_html(&m.label),
            ));
            let hidden = if i == 0 { "" } else { " hidden" };
            panes_html.push_str(&format!(
                r#"<section id="pane-{key}"{hidden}>{html}</section>"#,
                key = escape_html(&m.key),
                html = m.html,
            ));
        }
        tabs_html.push_str(
            r#"<button type="button" class="tab" data-tab="password">账号密码</button></div>"#,
        );
    }
    let pw_hidden = if has_ext { " hidden" } else { "" };
    let subtitle = if has_ext {
        "登录后凭证由控制面统一下发"
    } else {
        "使用企业账号登录，凭证将由控制面统一下发"
    };
    let switch_script = if has_ext {
        r#"<script>
(function () {
  var tabs = document.querySelectorAll('.tab');
  function show(which) {
    for (var i = 0; i < tabs.length; i += 1) {
      tabs[i].classList.toggle('active', tabs[i].getAttribute('data-tab') === which);
    }
    var panes = document.querySelectorAll('[id^="pane-"]');
    for (var j = 0; j < panes.length; j += 1) {
      panes[j].hidden = panes[j].id !== 'pane-' + which;
    }
  }
  for (var i = 0; i < tabs.length; i += 1) {
    (function (tab) {
      tab.addEventListener('click', function () { show(tab.getAttribute('data-tab')); });
    })(tabs[i]);
  }
})();
</script>"#
    } else {
        ""
    };

    Ok(format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Pointer 企业控制面 · 登录</title>
<style>
:root{{--bg:#FAFAFA;--fg:#18181B;--muted:#71717A;--line:#E5E5E8;--accent:#0071EB}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;
background:var(--bg);color:var(--fg);
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}}
.card{{width:380px;padding:32px;background:#fff;border:1px solid var(--line);border-radius:12px}}
h1{{margin:0 0 6px;font-size:18px}}
p.sub{{margin:0 0 22px;color:var(--muted);font-size:13px}}
p.err{{margin:0 0 12px;padding:8px 10px;border-radius:8px;background:#FDEAEA;color:#A12A2A;font-size:13px}}
label{{display:block;margin:14px 0 6px;font-size:13px;color:var(--muted)}}
input{{width:100%;padding:10px 12px;border:1px solid var(--line);border-radius:8px;font-size:14px}}
.row{{display:flex;gap:8px;align-items:center}}
.row input{{flex:1}}
.row img{{height:40px;border-radius:8px;cursor:pointer;border:1px solid var(--line)}}
button{{width:100%;margin-top:22px;padding:11px;border:0;border-radius:8px;
background:var(--accent);color:#fff;font-size:14px;cursor:pointer}}
.note{{margin-top:16px;color:var(--muted);font-size:12px;line-height:1.6}}
.sms{{display:flex;flex-direction:column;gap:8px}}
.sms button{{width:100%;margin:0}}
.sms .row button{{width:auto;padding:10px 12px;white-space:nowrap}}
.sms .err{{margin:0}}
.tabs{{display:flex;gap:8px;margin:0 0 18px;border-bottom:1px solid var(--line)}}
.tab{{width:auto;margin:0;padding:8px 2px;border:0;border-bottom:2px solid transparent;border-radius:0;
background:none;color:var(--muted);font-size:14px;cursor:pointer}}
.tab.active{{color:var(--accent);border-bottom-color:var(--accent)}}
#pane-ext{{margin-bottom:4px}}
</style></head>
<body><form class="card" method="post" action="/oauth/authorize">
<h1>登录 Pointer</h1>
<p class="sub">{subtitle}</p>
{error_html}
{tabs_html}
{panes_html}
<input type="hidden" name="client_id" value="{client_id}">
<input type="hidden" name="redirect_uri" value="{redirect_uri}">
<input type="hidden" name="state" value="{state_value}">
<input type="hidden" name="code_challenge" value="{challenge}">
<input type="hidden" name="captcha_id" value="{captcha_id}">
<div id="pane-password"{pw_hidden}>
<label for="username">账号</label>
<input id="username" name="username" autocomplete="username" autofocus required>
<label for="password">密码</label>
<input id="password" name="password" type="password" autocomplete="current-password" required>
<label for="captcha">验证码</label>
<div class="row">
<input id="captcha" name="captcha" autocomplete="off" maxlength="4" required>
<img src="{captcha_image}" alt="验证码" title="看不清？刷新页面换一张">
</div>
<button type="submit">登录</button>
<p class="note">首次登录后请立即修改初始密码。账号由管理员统一创建。</p>
</div>
</form>{switch_script}</body></html>"#,
        subtitle = subtitle,
        tabs_html = tabs_html,
        panes_html = panes_html,
        pw_hidden = pw_hidden,
        switch_script = switch_script,
        client_id = escape_html(&q.client_id),
        redirect_uri = escape_html(&q.redirect_uri),
        state_value = escape_html(q.state.as_deref().unwrap_or("")),
        challenge = escape_html(&q.code_challenge),
        captcha_id = escape_html(&captcha_id),
        captcha_image = captcha_image,
    ))
}

pub async fn submit(
    State(state): State<Shared>,
    Form(form): Form<AuthorizeForm>,
) -> ApiResult<Response> {
    let query = AuthorizeQuery {
        client_id: form.client_id.clone(),
        redirect_uri: form.redirect_uri.clone(),
        state: form.state.clone(),
        code_challenge: form.code_challenge.clone(),
        code_challenge_method: None,
    };
    validate(&state, &query).await?;

    // 验证码先于密码校验，且答错就换一张——撞库脚本连密码都试不到。
    if state.config.captcha_required
        && (form.captcha_id.trim().is_empty()
            || !crate::captcha::consume(&state.store, form.captcha_id.trim(), form.captcha.trim())
                .await
                .map_err(ApiError::internal)?)
    {
        let html = render_page(&state, &query, Some("验证码不正确或已过期，请重试")).await?;
        return Ok(Html(html).into_response());
    }

    let user = users::authenticate(
        &state.store,
        &state.config.tenant_id,
        &form.username,
        &form.password,
    )
    .await?;
    let Some(user) = user else {
        // 不抛 JSON：让员工能在同一页重试，不然换个验证码还得重开客户端。
        let html = render_page(&state, &query, Some("账号或密码错误")).await?;
        return Ok(Html(html).into_response());
    };

    let code = crate::oauth::code::issue(
        &state.store,
        &form.client_id,
        &user.id,
        &form.redirect_uri,
        &form.code_challenge,
    )
    .await
    .map_err(ApiError::internal)?;

    Ok(Redirect::to(&redirect_target(
        &form.redirect_uri,
        &code,
        form.state.as_deref(),
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_covers_quotes() {
        assert_eq!(escape_html(r#"<a href="x">"#), "&lt;a href=&quot;x&quot;&gt;");
        assert_eq!(escape_html("a&b"), "a&amp;b");
    }

    #[test]
    fn urlenc_keeps_unreserved_and_escapes_rest() {
        assert_eq!(urlenc("abc-_.~"), "abc-_.~");
        assert_eq!(urlenc("a b&c"), "a%20b%26c");
        assert_eq!(urlenc("中文"), "%E4%B8%AD%E6%96%87");
    }
}
