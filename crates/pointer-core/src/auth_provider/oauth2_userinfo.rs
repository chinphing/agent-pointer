//! `oauth2_userinfo` driver: authorization code → access token → userinfo.
//!
//! The whole exchange is driven by [`AuthProviderConfig`]: endpoint URLs, HTTP
//! methods, client auth style, parameter names, response pointers, and the
//! failure rule. Nothing here knows about a specific vendor — DingTalk is just
//! a config entry (camelCase token body, access token in
//! `x-acs-dingtalk-access-token`, subject at `/unionId`).

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};
use std::time::Duration;

use super::config::{
    AuthProviderConfig, AuthProviderKind, ClientAuth, ErrorRule, HttpMethod, ParamEncoding,
    TokenAuth,
};
use super::identity::{AuthCodeRequest, AuthProviderDriver, AuthorizeRequest, Identity};
use super::json_path;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const BODY_SNIPPET_LIMIT: usize = 200;

/// Token endpoint response, mapped through the configured JSON pointers.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub id_token: Option<String>,
    /// Full token response body.
    pub raw: Value,
}

/// Code-for-token + userinfo driver.
pub struct Oauth2UserinfoDriver {
    provider_id: String,
    config: AuthProviderConfig,
    http: reqwest::Client,
}

impl Oauth2UserinfoDriver {
    pub fn new(provider_id: impl Into<String>, config: AuthProviderConfig) -> Result<Self> {
        let provider_id = provider_id.into();
        if config.kind != AuthProviderKind::Oauth2Userinfo {
            bail!(
                "auth provider `{provider_id}`: kind `{}` is not served by this driver",
                config.kind.as_str()
            );
        }
        config.validate(&provider_id)?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build auth provider HTTP client")?;
        Ok(Self {
            provider_id,
            config,
            http,
        })
    }

    pub fn config(&self) -> &AuthProviderConfig {
        &self.config
    }

    /// Browser redirect target that starts the code flow.
    pub fn build_authorize_url(&self, request: &AuthorizeRequest<'_>) -> Result<String> {
        let cfg = &self.config.authorize;
        let base = cfg
            .url_opt()
            .ok_or_else(|| anyhow!("auth provider `{}`: authorize.url is required", self.provider_id))?;
        let mut params: Vec<(&str, String)> = vec![
            ("response_type", "code".to_string()),
            ("client_id", self.config.token.client_id.trim().to_string()),
            ("redirect_uri", request.redirect_uri.trim().to_string()),
            ("state", request.state.trim().to_string()),
            ("scope", cfg.scope.trim().to_string()),
        ];
        if cfg.pkce() {
            if let Some(challenge) = request.code_challenge.filter(|c| !c.trim().is_empty()) {
                params.push(("code_challenge", challenge.trim().to_string()));
                params.push(("code_challenge_method", "S256".to_string()));
            }
        }
        if let Some(nonce) = request.nonce.filter(|n| !n.trim().is_empty()) {
            params.push(("nonce", nonce.trim().to_string()));
        }
        for (name, value) in &cfg.extra_params {
            params.push((name.as_str(), value.clone()));
        }
        Ok(super::query::with_params(base, &params))
    }

    /// Exchange an authorization code for a token.
    pub async fn exchange_code(&self, request: &AuthCodeRequest<'_>) -> Result<TokenSet> {
        let code = request.code.trim();
        if code.is_empty() {
            bail!("auth provider `{}`: authorization code is empty", self.provider_id);
        }
        let cfg = &self.config.token;
        let url = cfg.url.trim();
        let mut params: Vec<(String, String)> = Vec::new();

        let grant_param = cfg.grant_type_param();
        if !grant_param.is_empty() {
            params.push((grant_param.to_string(), cfg.grant_type_value().to_string()));
        }
        push_param(&mut params, cfg.code_param(), code);
        match cfg.auth {
            ClientAuth::ClientSecretPost => {
                push_param(&mut params, cfg.client_id_param(), &cfg.client_id);
                push_param(&mut params, cfg.client_secret_param(), &cfg.client_secret);
            }
            ClientAuth::ClientSecretBasic => {}
            ClientAuth::None => push_param(&mut params, cfg.client_id_param(), &cfg.client_id),
        }
        let redirect_param = cfg.redirect_uri_param();
        if !redirect_param.is_empty() {
            push_param(&mut params, redirect_param, request.redirect_uri.trim());
        }
        let verifier_param = cfg.code_verifier_param();
        if !verifier_param.is_empty() {
            if let Some(verifier) = request.code_verifier.filter(|v| !v.trim().is_empty()) {
                push_param(&mut params, verifier_param, verifier);
            }
        }
        for (name, value) in &cfg.extra_params {
            push_param(&mut params, name, value);
        }

        let mut builder = match cfg.method() {
            HttpMethod::Get => self.http.get(url),
            HttpMethod::Post => self.http.post(url),
        };
        if cfg.auth == ClientAuth::ClientSecretBasic {
            builder = builder.basic_auth(&cfg.client_id, Some(&cfg.client_secret));
        }
        builder = match cfg.method() {
            // A body on GET is not portable; the token endpoint in query form.
            HttpMethod::Get => builder.query(&params),
            HttpMethod::Post => match cfg.encoding {
                ParamEncoding::Json => builder.json(&params_json(&params)),
                ParamEncoding::Form => builder.form(&params),
            },
        };

        let stage = "token";
        let (status, text) = self.send(builder, stage).await?;
        let body = evaluate_response(
            &self.provider_id,
            stage,
            status,
            &text,
            &cfg.error,
            &cfg.error_code_path,
            &cfg.error_message_path,
        )?;

        let access_token = json_path::as_string(&body, cfg.access_token_path())
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: token response has no access token at `{}`",
                    self.provider_id,
                    cfg.access_token_path()
                )
            })?;
        let refresh_token = non_empty(json_path::as_string(&body, &cfg.refresh_token_path));
        let expires_in = json_path::get(&body, &cfg.expires_in_path).and_then(|value| match value {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.trim().parse::<i64>().ok(),
            _ => None,
        });
        let id_token = non_empty(json_path::as_string(&body, &cfg.id_token_path));

        Ok(TokenSet {
            access_token,
            refresh_token,
            expires_in,
            id_token,
            raw: body,
        })
    }

    /// Read the identity for an access token already obtained from the provider.
    pub async fn fetch_userinfo(&self, access_token: &str) -> Result<Identity> {
        let token = access_token.trim();
        if token.is_empty() {
            bail!("auth provider `{}`: access token is empty", self.provider_id);
        }
        let cfg = &self.config.userinfo;
        let url = cfg.url.trim();
        let name = cfg.token_name();
        let value = format!("{}{}", cfg.token_prefix(), token);

        let mut builder = match cfg.method() {
            HttpMethod::Get => self.http.get(url),
            HttpMethod::Post => self.http.post(url),
        };
        builder = match cfg.token_auth {
            TokenAuth::Header => builder.header(name, value.as_str()),
            TokenAuth::Query => builder.query(&[(name, value.as_str())]),
            TokenAuth::Body => {
                let mut body = Map::new();
                body.insert(name.to_string(), Value::String(value.clone()));
                match cfg.body_encoding() {
                    ParamEncoding::Json => builder.json(&Value::Object(body)),
                    ParamEncoding::Form => builder.form(&[(name, value.as_str())]),
                }
            }
        };

        let stage = "userinfo";
        let (status, text) = self.send(builder, stage).await?;
        let body = evaluate_response(
            &self.provider_id,
            stage,
            status,
            &text,
            &cfg.error,
            &cfg.error_code_path,
            &cfg.error_message_path,
        )?;

        let subject_path = cfg.subject_path();
        let subject = json_path::as_string(&body, subject_path)
            .map(|subject| subject.trim().to_string())
            .filter(|subject| !subject.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: userinfo response has no subject at `{}`",
                    self.provider_id,
                    subject_path
                )
            })?;
        let issuer = if cfg.issuer_path.is_empty() {
            self.config.issuer_opt()
        } else {
            non_empty(json_path::as_string(&body, &cfg.issuer_path))
                .or_else(|| self.config.issuer_opt())
        };

        Ok(Identity {
            provider_id: self.provider_id.clone(),
            issuer,
            subject,
            claims: body,
        })
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
        stage: &str,
    ) -> Result<(reqwest::StatusCode, String)> {
        let response = builder.send().await.with_context(|| {
            format!(
                "auth provider `{}`: {stage} request could not be sent",
                self.provider_id
            )
        })?;
        let status = response.status();
        let text = response.text().await.with_context(|| {
            format!(
                "auth provider `{}`: {stage} response body could not be read",
                self.provider_id
            )
        })?;
        Ok((status, text))
    }
}

#[async_trait::async_trait]
impl AuthProviderDriver for Oauth2UserinfoDriver {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn kind(&self) -> AuthProviderKind {
        AuthProviderKind::Oauth2Userinfo
    }

    async fn authorize_url(&self, request: &AuthorizeRequest<'_>) -> Result<Option<String>> {
        self.build_authorize_url(request).map(Some)
    }

    async fn authenticate(&self, request: AuthCodeRequest<'_>) -> Result<Identity> {
        let token = self.exchange_code(&request).await?;
        self.fetch_userinfo(&token.access_token).await
    }
}

fn push_param(params: &mut Vec<(String, String)>, name: &str, value: &str) {
    if name.is_empty() || value.is_empty() {
        return;
    }
    params.push((name.to_string(), value.to_string()));
}

fn params_json(params: &[(String, String)]) -> Value {
    let mut map = Map::new();
    for (name, value) in params {
        map.insert(name.clone(), Value::String(value.clone()));
    }
    Value::Object(map)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Status + [`ErrorRule`] gate. Returns the parsed body on success.
fn evaluate_response(
    provider_id: &str,
    stage: &str,
    status: reqwest::StatusCode,
    text: &str,
    rule: &ErrorRule,
    code_path: &str,
    message_path: &str,
) -> Result<Value> {
    let body: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let failure = if !status.is_success() {
        Some(format!("HTTP {}", status.as_u16()))
    } else {
        rule_failure(rule, &body)
    };
    if let Some(reason) = failure {
        let detail = describe_error(&body, text, code_path, message_path);
        bail!("auth provider `{provider_id}`: {stage} request failed ({reason}){detail}");
    }
    if body.is_null() {
        bail!(
            "auth provider `{provider_id}`: {stage} response is not JSON: {}",
            snippet(text)
        );
    }
    Ok(body)
}

fn rule_failure(rule: &ErrorRule, body: &Value) -> Option<String> {
    match rule {
        ErrorRule::HttpStatus => None,
        ErrorRule::PathNotEqual { path, ok_value } => match json_path::as_string(body, path) {
            Some(actual) if actual == *ok_value => None,
            Some(actual) => Some(format!("`{path}` = `{actual}`, expected `{ok_value}`")),
            None => Some(format!("`{path}` is missing")),
        },
        ErrorRule::PathPresent { path } => json_path::get(body, path)
            .filter(|value| is_truthy(value))
            .map(|value| format!("`{path}` = `{}`", compact(value))),
    }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(true),
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn compact(value: &Value) -> String {
    match value {
        Value::String(text) => snippet(text),
        other => snippet(&other.to_string()),
    }
}

fn describe_error(body: &Value, text: &str, code_path: &str, message_path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !code_path.is_empty() {
        if let Some(code) = json_path::as_string(body, code_path) {
            parts.push(format!("code={}", snippet(&code)));
        }
    }
    if !message_path.is_empty() {
        if let Some(message) = json_path::as_string(body, message_path) {
            parts.push(format!("message={}", snippet(&message)));
        }
    }
    if !parts.is_empty() {
        return format!(": {}", parts.join(" "));
    }
    let text = snippet(text);
    if text.is_empty() {
        String::new()
    } else {
        format!(": {text}")
    }
}

fn snippet(text: &str) -> String {
    let collapsed = text.trim().replace(['\n', '\r'], " ");
    if collapsed.chars().count() <= BODY_SNIPPET_LIMIT {
        return collapsed;
    }
    let head: String = collapsed.chars().take(BODY_SNIPPET_LIMIT).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// DingTalk, wired to a local mock server.
    fn dingtalk_config(server: &MockServer) -> AuthProviderConfig {
        toml::from_str(&format!(
            r#"
kind = "oauth2_userinfo"
display_name = "钉钉"
issuer = "https://login.dingtalk.com"

[authorize]
url = "{base}/oauth2/auth"
scope = "openid"

[token]
url = "{base}/v1.0/oauth2/userAccessToken"
client_id = "ding-client"
client_secret = "ding-secret"
encoding = "json"
grant_type_param = "grantType"
client_id_param = "clientId"
client_secret_param = "clientSecret"
redirect_uri_param = ""
access_token_path = "/accessToken"
refresh_token_path = "/refreshToken"
expires_in_path = "/expireIn"

[userinfo]
url = "{base}/v1.0/contact/users/me"
token_auth = "header"
token_name = "x-acs-dingtalk-access-token"
token_prefix = ""
subject_path = "/unionId"
"#,
            base = server.uri()
        ))
        .expect("dingtalk config parses")
    }

    fn request<'a>(code: &'a str, redirect_uri: &'a str) -> AuthCodeRequest<'a> {
        AuthCodeRequest {
            code,
            redirect_uri,
            code_verifier: None,
            nonce: None,
        }
    }

    #[tokio::test]
    async fn authorize_url_carries_pkce_state_and_scope() {
        let server = MockServer::start().await;
        let driver =
            Oauth2UserinfoDriver::new("dingtalk", dingtalk_config(&server)).expect("driver builds");

        let url = driver
            .authorize_url(&AuthorizeRequest {
                redirect_uri: "https://pointer.example.com/api/auth/callback/dingtalk",
                state: "state-1",
                code_challenge: Some("challenge-1"),
                nonce: Some("nonce-1"),
            })
            .await
            .expect("authorize url builds")
            .expect("provider has an authorize url");

        assert!(url.starts_with(&format!("{}/oauth2/auth?", server.uri())), "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("client_id=ding-client"), "{url}");
        assert!(url.contains("state=state-1"), "{url}");
        assert!(url.contains("scope=openid"), "{url}");
        assert!(url.contains("nonce=nonce-1"), "{url}");
        assert!(url.contains("code_challenge=challenge-1"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        // The redirect_uri is percent-encoded, not spliced in raw.
        assert!(
            url.contains(
                "redirect_uri=https%3A%2F%2Fpointer.example.com%2Fapi%2Fauth%2Fcallback%2Fdingtalk"
            ),
            "{url}"
        );
    }

    #[tokio::test]
    async fn authorize_url_omits_pkce_when_disabled() {
        let server = MockServer::start().await;
        let mut config = dingtalk_config(&server);
        config.authorize.pkce = Some(false);
        let driver = Oauth2UserinfoDriver::new("dingtalk", config).expect("driver builds");

        let url = driver
            .authorize_url(&AuthorizeRequest {
                redirect_uri: "https://pointer.example.com/cb",
                state: "s",
                code_challenge: Some("challenge-1"),
                nonce: None,
            })
            .await
            .expect("authorize url builds")
            .expect("provider has an authorize url");
        assert!(!url.contains("code_challenge"), "{url}");
    }

    #[tokio::test]
    async fn dingtalk_flow_returns_union_id() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1.0/oauth2/userAccessToken"))
            .and(body_partial_json(json!({
                "grantType": "authorization_code",
                "clientId": "ding-client",
                "clientSecret": "ding-secret",
                "code": "auth-code-1"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "accessToken": "tok-1",
                "refreshToken": "ref-1",
                "expireIn": 7200
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1.0/contact/users/me"))
            .and(header("x-acs-dingtalk-access-token", "tok-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "nick": "张三",
                "unionId": "union-1",
                "openId": "open-1"
            })))
            .mount(&server)
            .await;

        let driver = Oauth2UserinfoDriver::new("dingtalk", dingtalk_config(&server))
            .expect("driver builds");
        let identity = driver
            .authenticate(request("auth-code-1", "https://pointer.example.com/cb"))
            .await
            .expect("dingtalk login succeeds");

        assert_eq!(identity.provider_id, "dingtalk");
        assert_eq!(identity.subject, "union-1");
        assert_eq!(
            identity.issuer.as_deref(),
            Some("https://login.dingtalk.com")
        );
        assert_eq!(identity.claims["nick"], json!("张三"));

        // Token mapping goes through the configured pointers.
        let token = driver
            .exchange_code(&request("auth-code-1", "https://pointer.example.com/cb"))
            .await
            .expect("token exchange succeeds");
        assert_eq!(token.access_token, "tok-1");
        assert_eq!(token.refresh_token.as_deref(), Some("ref-1"));
        assert_eq!(token.expires_in, Some(7200));
        assert_eq!(token.id_token, None);
    }

    #[tokio::test]
    async fn http_error_status_is_reported_with_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/oauth2/userAccessToken"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "code": "invalidGrant",
                "message": "code is expired"
            })))
            .mount(&server)
            .await;

        let mut config = dingtalk_config(&server);
        config.token.error_code_path = "/code".into();
        config.token.error_message_path = "/message".into();
        let driver = Oauth2UserinfoDriver::new("dingtalk", config).expect("driver builds");

        let err = driver
            .authenticate(request("expired", "https://pointer.example.com/cb"))
            .await
            .expect_err("400 fails the login");
        let text = err.to_string();
        assert!(text.contains("HTTP 400"), "{text}");
        assert!(text.contains("code=invalidGrant"), "{text}");
        assert!(text.contains("message=code is expired"), "{text}");
    }

    #[tokio::test]
    async fn error_rule_detects_failure_inside_http_200() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/oauth2/userAccessToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errcode": 40014,
                "errmsg": "invalid code"
            })))
            .mount(&server)
            .await;

        let mut config = dingtalk_config(&server);
        config.token.error = ErrorRule::PathNotEqual {
            path: "/errcode".into(),
            ok_value: "0".into(),
        };
        config.token.error_code_path = "/errcode".into();
        config.token.error_message_path = "/errmsg".into();
        let driver = Oauth2UserinfoDriver::new("dingtalk", config).expect("driver builds");

        let err = driver
            .authenticate(request("bad", "https://pointer.example.com/cb"))
            .await
            .expect_err("errcode != 0 fails the login");
        let text = err.to_string();
        assert!(text.contains("/errcode"), "{text}");
        assert!(text.contains("40014"), "{text}");
        assert!(text.contains("invalid code"), "{text}");
    }

    #[tokio::test]
    async fn query_placement_and_nested_subject() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/oauth/userinfo"))
            .and(query_param("access_token", "tok-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"user": {"id": 4242}, "issuer": "https://idp.example.com"},
                "error": null
            })))
            .mount(&server)
            .await;

        let config: AuthProviderConfig = toml::from_str(&format!(
            r#"
kind = "oauth2_userinfo"
issuer = "https://fallback.example.com"

[authorize]
url = "{base}/oauth/authorize"

[token]
url = "{base}/oauth/token"
client_id = "id"
client_secret = "secret"
auth = "client_secret_basic"
encoding = "form"

[userinfo]
url = "{base}/oauth/userinfo"
token_auth = "query"
subject_path = "/data/user/id"
issuer_path = "/data/issuer"
error = {{ kind = "path_present", path = "/error" }}
"#,
            base = server.uri()
        ))
        .expect("config parses");

        let driver = Oauth2UserinfoDriver::new("idp", config).expect("driver builds");
        let identity = driver
            .fetch_userinfo("tok-2")
            .await
            .expect("userinfo succeeds");
        assert_eq!(identity.subject, "4242");
        assert_eq!(identity.issuer.as_deref(), Some("https://idp.example.com"));
        assert_eq!(identity.claims["data"]["user"]["id"], json!(4242));
    }

    #[tokio::test]
    async fn body_placement_and_form_encoding_send_expected_params() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(header("authorization", "Basic aWQ6c2VjcmV0"))
            .and(wiremock::matchers::body_string_contains(
                "grant_type=authorization_code",
            ))
            .and(wiremock::matchers::body_string_contains("code=abc"))
            .and(wiremock::matchers::body_string_contains(
                "redirect_uri=https%3A%2F%2Fpointer.example.com%2Fcb",
            ))
            .and(wiremock::matchers::body_string_contains(
                "code_verifier=verifier-1",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "tok-3"
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/userinfo"))
            .and(wiremock::matchers::body_string_contains("access_token=tok-3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "sub": "user-9"
            })))
            .mount(&server)
            .await;

        let config: AuthProviderConfig = toml::from_str(&format!(
            r#"
kind = "oauth2_userinfo"

[authorize]
url = "{base}/authorize"

[token]
url = "{base}/token"
client_id = "id"
client_secret = "secret"
auth = "client_secret_basic"
encoding = "form"

[userinfo]
url = "{base}/userinfo"
method = "post"
token_auth = "body"
"#,
            base = server.uri()
        ))
        .expect("config parses");

        let driver = Oauth2UserinfoDriver::new("idp", config).expect("driver builds");
        let identity = driver
            .authenticate(AuthCodeRequest {
                code: "abc",
                redirect_uri: "https://pointer.example.com/cb",
                code_verifier: Some("verifier-1"),
                nonce: None,
            })
            .await
            .expect("login succeeds");
        assert_eq!(identity.subject, "user-9");
        // No issuer configured and none in the response.
        assert_eq!(identity.issuer, None);
    }

    #[tokio::test]
    async fn missing_subject_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"nick": "no id"})))
            .mount(&server)
            .await;

        let config: AuthProviderConfig = toml::from_str(&format!(
            r#"
kind = "oauth2_userinfo"

[authorize]
url = "{base}/authorize"

[token]
url = "{base}/token"
client_id = "id"
client_secret = "secret"

[userinfo]
url = "{base}/userinfo"
subject_path = "/unionId"
"#,
            base = server.uri()
        ))
        .expect("config parses");

        let driver = Oauth2UserinfoDriver::new("idp", config).expect("driver builds");
        let err = driver
            .fetch_userinfo("tok")
            .await
            .expect_err("missing subject fails");
        assert!(err.to_string().contains("/unionId"), "{err}");
    }

    #[tokio::test]
    async fn non_json_success_body_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>gateway</html>"))
            .mount(&server)
            .await;

        let config: AuthProviderConfig = toml::from_str(&format!(
            r#"
kind = "oauth2_userinfo"

[authorize]
url = "{base}/authorize"

[token]
url = "{base}/token"
client_id = "id"
client_secret = "secret"

[userinfo]
url = "{base}/userinfo"
"#,
            base = server.uri()
        ))
        .expect("config parses");

        let driver = Oauth2UserinfoDriver::new("idp", config).expect("driver builds");
        let err = driver
            .fetch_userinfo("tok")
            .await
            .expect_err("non-JSON fails");
        assert!(err.to_string().contains("not JSON"), "{err}");
    }
}
