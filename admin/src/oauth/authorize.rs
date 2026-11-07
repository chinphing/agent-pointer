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
    Query(q): Query<AuthorizeQuery>,
) -> ApiResult<Response> {
    validate(&state, &q).await?;

    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Pointer 企业控制面 · 登录</title>
<style>
:root{{--bg:#f6f7f9;--fg:#1f2328;--muted:#6b7280;--line:#d9dde3;--accent:#185FA5}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;
background:var(--bg);color:var(--fg);
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif}}
.card{{width:380px;padding:32px;background:#fff;border:1px solid var(--line);border-radius:12px}}
h1{{margin:0 0 6px;font-size:18px}}
p.sub{{margin:0 0 22px;color:var(--muted);font-size:13px}}
label{{display:block;margin:14px 0 6px;font-size:13px;color:var(--muted)}}
input{{width:100%;padding:10px 12px;border:1px solid var(--line);border-radius:8px;font-size:14px}}
button{{width:100%;margin-top:22px;padding:11px;border:0;border-radius:8px;
background:var(--accent);color:#fff;font-size:14px;cursor:pointer}}
.note{{margin-top:16px;color:var(--muted);font-size:12px;line-height:1.6}}
</style></head>
<body><form class="card" method="post" action="/oauth/authorize">
<h1>登录 Pointer</h1>
<p class="sub">使用企业账号登录，凭证将由控制面统一下发</p>
<input type="hidden" name="client_id" value="{client_id}">
<input type="hidden" name="redirect_uri" value="{redirect_uri}">
<input type="hidden" name="state" value="{state_value}">
<input type="hidden" name="code_challenge" value="{challenge}">
<label for="username">账号</label>
<input id="username" name="username" autocomplete="username" autofocus required>
<label for="password">密码</label>
<input id="password" name="password" type="password" autocomplete="current-password" required>
<button type="submit">登录</button>
<p class="note">首次登录后请立即修改初始密码。账号由管理员统一创建。</p>
</form></body></html>"#,
        client_id = escape_html(&q.client_id),
        redirect_uri = escape_html(&q.redirect_uri),
        state_value = escape_html(q.state.as_deref().unwrap_or("")),
        challenge = escape_html(&q.code_challenge),
    );
    Ok(Html(html).into_response())
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

    let user = users::authenticate(
        &state.store,
        &state.config.tenant_id,
        &form.username,
        &form.password,
    )
    .await?;
    let Some(user) = user else {
        return Err(ApiError::Unauthorized);
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

    let mut target = format!("{}?code={}", form.redirect_uri, urlenc(&code));
    if let Some(st) = form.state.as_deref().filter(|s| !s.is_empty()) {
        target.push_str(&format!("&state={}", urlenc(st)));
    }
    Ok(Redirect::to(&target).into_response())
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
