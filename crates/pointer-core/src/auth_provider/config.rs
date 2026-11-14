//! Provider configuration model: `[auth.providers.<id>]` in `pointer-server.toml`.
//!
//! Four things are configurable per provider, and nothing else:
//!
//! 1. `token_auth` — where the access token goes on the userinfo request
//!    (`header` / `query` / `body`).
//! 2. Parameter placement and naming — endpoint URLs, HTTP method, client auth
//!    style, and the exact parameter names (`grantType`, `clientId`, …).
//! 3. Field paths — restricted JSON pointers to the access token and the
//!    subject (`/accessToken`, `/unionId`, `/data/user/id`).
//! 4. Error detection — a closed enum ([`ErrorRule`]): non-2xx status, a path
//!    that must equal a value, or a path that must be absent/zero.
//!
//! Everything is declarative. No scripts, no expression language.

use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::json_path;

/// How the access token is attached to the userinfo request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAuth {
    /// Header, `Authorization: Bearer <token>` unless renamed. Default.
    #[default]
    Header,
    /// Query parameter, `?access_token=<token>` unless renamed.
    Query,
    /// Form / JSON body field; requires `method = "post"`.
    Body,
}

/// How the client authenticates on the token request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuth {
    /// `client_id` + `client_secret` in the body (RFC 6749 §2.3.1). Default.
    #[default]
    ClientSecretPost,
    /// HTTP Basic with client id / secret.
    ClientSecretBasic,
    /// Public client (PKCE only): no secret is sent.
    None,
}

/// Wire encoding for request parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamEncoding {
    #[default]
    Json,
    Form,
}

/// HTTP method for a provider endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Get,
    Post,
}

/// Restricted failure detection for a provider HTTP response.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ErrorRule {
    /// Any non-2xx status is a failure. Default.
    #[default]
    HttpStatus,
    /// `path` must exist and equal `ok_value`; anything else fails
    /// (DingTalk legacy: `{ kind = "path_not_equal", path = "/errcode", ok_value = "0" }`).
    PathNotEqual { path: String, ok_value: String },
    /// A `path` that resolves to a non-empty / non-zero value fails
    /// (`{ kind = "path_present", path = "/error" }`).
    PathPresent { path: String },
}

impl ErrorRule {
    fn validate(&self, what: &str) -> Result<()> {
        match self {
            ErrorRule::HttpStatus => Ok(()),
            ErrorRule::PathNotEqual { path, ok_value } => {
                if path.trim().is_empty() {
                    bail!("{what}: error rule `path_not_equal` requires `path`");
                }
                if ok_value.is_empty() {
                    bail!("{what}: error rule `path_not_equal` requires `ok_value`");
                }
                json_path::validate(path).map_err(|e| anyhow::anyhow!("{what}: {e}"))
            }
            ErrorRule::PathPresent { path } => {
                if path.trim().is_empty() {
                    bail!("{what}: error rule `path_present` requires `path`");
                }
                json_path::validate(path).map_err(|e| anyhow::anyhow!("{what}: {e}"))
            }
        }
    }
}

/// Authorize endpoint: the browser hop that starts the code flow.
///
/// Required for `oauth2_userinfo` (the provider never publishes it for us).
/// Optional for `oidc`, where discovery supplies the URL; when set, `scope` /
/// `extra_params` still apply on top of the discovered endpoint.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuthorizeEndpointConfig {
    pub url: String,
    /// Space-separated scope, sent only when non-empty.
    #[serde(default)]
    pub scope: String,
    /// Send a PKCE `code_challenge` (default true).
    #[serde(default)]
    pub pkce: Option<bool>,
    /// Static extra query parameters (`audience`, `prompt`, …).
    #[serde(default)]
    pub extra_params: BTreeMap<String, String>,
}

impl AuthorizeEndpointConfig {
    pub fn pkce(&self) -> bool {
        self.pkce.unwrap_or(true)
    }

    pub fn url_opt(&self) -> Option<&str> {
        let url = self.url.trim();
        if url.is_empty() {
            None
        } else {
            Some(url)
        }
    }

    fn validate(&self, what: &str, require_url: bool) -> Result<()> {
        match self.url_opt() {
            Some(url) => validate_http_url(&format!("{what}.url"), url)?,
            None if require_url => bail!("{what}.url is required"),
            None => {}
        }
        if self.extra_params.keys().any(|k| k.trim().is_empty()) {
            bail!("{what}.extra_params contains an empty parameter name");
        }
        Ok(())
    }
}

/// OpenID Connect driver settings (`kind = "oidc"`).
///
/// The endpoints come from `{issuer}/.well-known/openid-configuration`; the ID
/// token is verified against the published JWKS before any claim is trusted.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OidcConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    /// Client authentication style on the token request.
    #[serde(default)]
    pub auth: ClientAuth,
    /// Extra scopes; `openid` is always requested.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Discovery document URL; defaults to `{issuer}/.well-known/openid-configuration`.
    #[serde(default)]
    pub discovery_url: String,
    /// Expected `aud` claim; defaults to `client_id`.
    #[serde(default)]
    pub audience: String,
    /// Restricted pointer to the subject inside the ID token claims. Default `/sub`.
    #[serde(default)]
    pub subject_path: Option<String>,
    /// Accepted `alg` values. Default `RS256`.
    #[serde(default)]
    pub algs: Vec<String>,
    /// Clock skew tolerance in seconds for `exp` / `iat`. Default 120.
    #[serde(default)]
    pub clock_skew_secs: Option<u64>,
}

impl OidcConfig {
    /// `openid` first, then configured scopes, de-duplicated.
    pub fn scopes(&self) -> Vec<String> {
        let mut out = vec!["openid".to_string()];
        for scope in &self.scopes {
            for part in scope.split_whitespace() {
                if !out.iter().any(|existing| existing == part) {
                    out.push(part.to_string());
                }
            }
        }
        out
    }

    /// Discovery URL: explicit override, else the issuer's well-known document.
    pub fn discovery_url(&self, issuer: &str) -> Option<String> {
        let explicit = self.discovery_url.trim();
        if !explicit.is_empty() {
            return Some(explicit.to_string());
        }
        let issuer = issuer.trim().trim_end_matches('/');
        if issuer.is_empty() {
            None
        } else {
            Some(format!("{issuer}/.well-known/openid-configuration"))
        }
    }

    pub fn audience(&self) -> String {
        let audience = self.audience.trim();
        if audience.is_empty() {
            self.client_id.trim().to_string()
        } else {
            audience.to_string()
        }
    }

    pub fn subject_path(&self) -> &str {
        self.subject_path.as_deref().unwrap_or("/sub")
    }

    /// Accepted signing algorithms, upper-cased. Default `RS256`.
    pub fn algs(&self) -> Vec<String> {
        let algs: Vec<String> = self
            .algs
            .iter()
            .map(|alg| alg.trim().to_ascii_uppercase())
            .filter(|alg| !alg.is_empty())
            .collect();
        if algs.is_empty() {
            vec!["RS256".to_string()]
        } else {
            algs
        }
    }

    pub fn clock_skew_secs(&self) -> u64 {
        self.clock_skew_secs.unwrap_or(120)
    }

    fn validate(&self, what: &str, issuer: &str) -> Result<()> {
        if issuer.trim().is_empty() {
            bail!("{what}: `issuer` is required for kind `oidc`");
        }
        validate_http_url(&format!("{what}: issuer"), issuer)?;
        if self.discovery_url.trim().is_empty() {
            if self.discovery_url(issuer).is_none() {
                bail!("{what}.discovery_url is required when `issuer` is empty");
            }
        } else {
            validate_http_url(&format!("{what}.discovery_url"), &self.discovery_url)?;
        }
        if self.client_id.trim().is_empty() {
            bail!("{what}.client_id is required");
        }
        if self.auth != ClientAuth::None && self.client_secret.trim().is_empty() {
            bail!("{what}.client_secret is required (use auth = \"none\" for a public client)");
        }
        if self.subject_path().trim().is_empty() {
            bail!("{what}.subject_path must not be empty");
        }
        validate_optional_path(&format!("{what}.subject_path"), Some(self.subject_path()))?;
        if let Some(bad) = self.algs().iter().find(|alg| !is_supported_alg(alg)) {
            bail!(
                "{what}.algs contains `{bad}` (supported: {} )",
                SUPPORTED_ALGS.join(", ")
            );
        }
        Ok(())
    }
}

/// Signing algorithms the driver can verify. Everything else is rejected at
/// startup instead of failing at login time.
pub const SUPPORTED_ALGS: [&str; 3] = ["RS256", "RS384", "RS512"];

fn is_supported_alg(alg: &str) -> bool {
    SUPPORTED_ALGS.contains(&alg)
}

/// Token endpoint: authorization code → access token.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TokenEndpointConfig {
    pub url: String,
    #[serde(default)]
    pub method: Option<HttpMethod>,
    #[serde(default)]
    pub auth: ClientAuth,
    /// Body encoding for `method = "post"` (`json` default, `form` for RFC 6749).
    #[serde(default)]
    pub encoding: ParamEncoding,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,

    // Parameter names. `None` → OAuth2 default; `Some("")` → omit the parameter.
    #[serde(default)]
    pub grant_type_param: Option<String>,
    #[serde(default)]
    pub grant_type_value: Option<String>,
    #[serde(default)]
    pub code_param: Option<String>,
    #[serde(default)]
    pub client_id_param: Option<String>,
    #[serde(default)]
    pub client_secret_param: Option<String>,
    #[serde(default)]
    pub redirect_uri_param: Option<String>,
    #[serde(default)]
    pub code_verifier_param: Option<String>,
    /// Static extra parameters (audience, scope, …).
    #[serde(default)]
    pub extra_params: BTreeMap<String, String>,

    // Response field paths (restricted JSON pointers).
    #[serde(default)]
    pub access_token_path: Option<String>,
    #[serde(default)]
    pub refresh_token_path: String,
    #[serde(default)]
    pub expires_in_path: String,
    #[serde(default)]
    pub id_token_path: String,

    #[serde(default)]
    pub error: ErrorRule,
    #[serde(default)]
    pub error_code_path: String,
    #[serde(default)]
    pub error_message_path: String,
}

impl TokenEndpointConfig {
    pub fn method(&self) -> HttpMethod {
        self.method.unwrap_or(HttpMethod::Post)
    }

    pub fn grant_type_param(&self) -> &str {
        self.grant_type_param.as_deref().unwrap_or("grant_type")
    }

    pub fn grant_type_value(&self) -> &str {
        self.grant_type_value
            .as_deref()
            .unwrap_or("authorization_code")
    }

    pub fn code_param(&self) -> &str {
        self.code_param.as_deref().unwrap_or("code")
    }

    pub fn client_id_param(&self) -> &str {
        self.client_id_param.as_deref().unwrap_or("client_id")
    }

    pub fn client_secret_param(&self) -> &str {
        self.client_secret_param.as_deref().unwrap_or("client_secret")
    }

    /// Empty string means "do not send a redirect_uri".
    pub fn redirect_uri_param(&self) -> &str {
        self.redirect_uri_param.as_deref().unwrap_or("redirect_uri")
    }

    /// Empty string means "do not send a PKCE verifier".
    pub fn code_verifier_param(&self) -> &str {
        self.code_verifier_param
            .as_deref()
            .unwrap_or("code_verifier")
    }

    pub fn access_token_path(&self) -> &str {
        self.access_token_path.as_deref().unwrap_or("/access_token")
    }

    fn validate(&self, what: &str) -> Result<()> {
        validate_http_url(&format!("{what}.url"), &self.url)?;
        if self.auth != ClientAuth::None && self.client_id.trim().is_empty() {
            bail!("{what}.client_id is required");
        }
        if self.auth != ClientAuth::None && self.client_secret.trim().is_empty() {
            bail!("{what}.client_secret is required (use auth = \"none\" for a public client)");
        }
        if self.code_param().is_empty() {
            bail!("{what}.code_param must not be empty");
        }
        validate_optional_path(&format!("{what}.access_token_path"), Some(self.access_token_path()))?;
        validate_optional_path(&format!("{what}.refresh_token_path"), Some(&self.refresh_token_path))?;
        validate_optional_path(&format!("{what}.expires_in_path"), Some(&self.expires_in_path))?;
        validate_optional_path(&format!("{what}.id_token_path"), Some(&self.id_token_path))?;
        validate_optional_path(&format!("{what}.error_code_path"), Some(&self.error_code_path))?;
        validate_optional_path(&format!("{what}.error_message_path"), Some(&self.error_message_path))?;
        if self.extra_params.keys().any(|k| k.trim().is_empty()) {
            bail!("{what}.extra_params contains an empty parameter name");
        }
        self.error.validate(what)
    }
}

/// Userinfo endpoint: access token → identity claims.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserinfoEndpointConfig {
    pub url: String,
    #[serde(default)]
    pub method: Option<HttpMethod>,
    #[serde(default)]
    pub token_auth: TokenAuth,
    /// Header / query / body field name. `None` → per-`token_auth` default.
    #[serde(default)]
    pub token_name: Option<String>,
    /// Prefix before the token. `None` → `Bearer ` for headers, nothing otherwise;
    /// `Some("")` sends the raw token (DingTalk `x-acs-dingtalk-access-token`).
    #[serde(default)]
    pub token_prefix: Option<String>,
    /// Only used when `token_auth = "body"`; defaults to `form`.
    #[serde(default)]
    pub encoding: Option<ParamEncoding>,
    /// Restricted pointer to the stable subject (DingTalk: `/unionId`). Default `/sub`.
    #[serde(default)]
    pub subject_path: Option<String>,
    /// Optional pointer to an issuer claim; falls back to the provider `issuer`.
    #[serde(default)]
    pub issuer_path: String,

    #[serde(default)]
    pub error: ErrorRule,
    #[serde(default)]
    pub error_code_path: String,
    #[serde(default)]
    pub error_message_path: String,
}

impl UserinfoEndpointConfig {
    pub fn method(&self) -> HttpMethod {
        self.method.unwrap_or(HttpMethod::Get)
    }

    pub fn token_name(&self) -> &str {
        self.token_name.as_deref().unwrap_or(match self.token_auth {
            TokenAuth::Header => "Authorization",
            TokenAuth::Query | TokenAuth::Body => "access_token",
        })
    }

    pub fn token_prefix(&self) -> &str {
        self.token_prefix.as_deref().unwrap_or(match self.token_auth {
            TokenAuth::Header => "Bearer ",
            TokenAuth::Query | TokenAuth::Body => "",
        })
    }

    pub fn body_encoding(&self) -> ParamEncoding {
        self.encoding.unwrap_or(ParamEncoding::Form)
    }

    pub fn subject_path(&self) -> &str {
        self.subject_path.as_deref().unwrap_or("/sub")
    }

    fn validate(&self, what: &str) -> Result<()> {
        validate_http_url(&format!("{what}.url"), &self.url)?;
        if self.token_name().trim().is_empty() {
            bail!("{what}.token_name must not be empty");
        }
        if self.token_auth == TokenAuth::Body && self.method() != HttpMethod::Post {
            bail!("{what}: token_auth = \"body\" requires method = \"post\"");
        }
        if self.subject_path().trim().is_empty() {
            bail!("{what}.subject_path must not be empty");
        }
        validate_optional_path(&format!("{what}.subject_path"), Some(self.subject_path()))?;
        validate_optional_path(&format!("{what}.issuer_path"), Some(&self.issuer_path))?;
        validate_optional_path(&format!("{what}.error_code_path"), Some(&self.error_code_path))?;
        validate_optional_path(&format!("{what}.error_message_path"), Some(&self.error_message_path))?;
        self.error.validate(what)
    }
}

/// Supported driver families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProviderKind {
    /// Cloud control-plane OAuth (managed deployments). Handled by the server
    /// layer — it yields platform tokens, not a bare [`super::Identity`].
    Platform,
    /// Local username + password form (standalone deployments).
    LocalPassword,
    /// Inbound SSO ticket minted by a portal: signed JWT or opaque lookup.
    TokenLookup,
    /// Exchange the code for a token, then read identity from a userinfo endpoint.
    Oauth2Userinfo,
    /// OpenID Connect: discovery + JWKS-verified ID token.
    Oidc,
}

impl AuthProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthProviderKind::Platform => "platform",
            AuthProviderKind::LocalPassword => "local_password",
            AuthProviderKind::TokenLookup => "token_lookup",
            AuthProviderKind::Oauth2Userinfo => "oauth2_userinfo",
            AuthProviderKind::Oidc => "oidc",
        }
    }

    /// Whether this kind is entered by a browser redirect we build.
    pub fn is_redirect(self) -> bool {
        matches!(
            self,
            AuthProviderKind::Platform | AuthProviderKind::Oauth2Userinfo | AuthProviderKind::Oidc
        )
    }

    /// Whether a login through this kind must pass the access allowlist.
    ///
    /// `local_password` is the operator's own break-glass account and is exempt;
    /// every federated identity is checked against `server_access::is_user_allowed`.
    pub fn requires_allowlist(self) -> bool {
        !matches!(self, AuthProviderKind::LocalPassword)
    }
}

/// Signed-ticket verification for `token_lookup` (the portal shares a secret).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TicketSigningConfig {
    pub secret: String,
    /// Previous secret, still accepted during rotation.
    #[serde(default)]
    pub secret_prev: String,
    /// Expected `audience` claim; empty means "do not check".
    #[serde(default)]
    pub audience: String,
}

/// Server-to-server verification of an opaque ticket.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TokenLookupEndpoint {
    pub url: String,
    #[serde(default)]
    pub method: Option<HttpMethod>,
    /// Name of the ticket parameter sent to the lookup endpoint. Default `ticket`.
    #[serde(default)]
    pub param: String,
    /// Optional bearer token for the lookup call.
    #[serde(default)]
    pub token: String,
    /// Encoding for `method = "post"`. Default `form`.
    #[serde(default)]
    pub encoding: Option<ParamEncoding>,
}

impl TokenLookupEndpoint {
    pub fn url_opt(&self) -> Option<&str> {
        let url = self.url.trim();
        if url.is_empty() {
            None
        } else {
            Some(url)
        }
    }

    pub fn method(&self) -> HttpMethod {
        self.method.unwrap_or(HttpMethod::Get)
    }

    pub fn param(&self) -> &str {
        let param = self.param.trim();
        if param.is_empty() {
            "ticket"
        } else {
            param
        }
    }

    pub fn body_encoding(&self) -> ParamEncoding {
        self.encoding.unwrap_or(ParamEncoding::Form)
    }
}

/// `kind = "token_lookup"`: the portal sends the browser back with a ticket.
///
/// Two verification modes, picked by config:
/// 1. `lookup.url` set → call the IdP server-to-server and map its response.
/// 2. otherwise → verify a signed ticket (HS256 JWT) against `signing.secret`.
///
/// Claim/response fields are restricted JSON pointers, and every redeemed
/// ticket is recorded once per provider so it cannot be replayed.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TokenLookupConfig {
    /// Query parameter carrying the ticket. Default `ticket`.
    #[serde(default)]
    pub ticket_param: String,
    #[serde(default)]
    pub lookup: TokenLookupEndpoint,
    #[serde(default)]
    pub signing: TicketSigningConfig,
    /// Restricted pointer to the stable subject. Default `/sub`.
    #[serde(default)]
    pub subject_path: Option<String>,
    /// Optional pointer to a display name.
    #[serde(default)]
    pub nickname_path: String,
    /// Optional pointer to the issuer claim; falls back to the provider `issuer`.
    #[serde(default)]
    pub issuer_path: String,
    #[serde(default)]
    pub audience_path: String,
    #[serde(default)]
    pub expires_at_path: String,
    #[serde(default)]
    pub issued_at_path: String,
    /// Ticket id used for replay protection. Default `/jti`.
    #[serde(default)]
    pub ticket_id_path: String,
    /// Clock skew tolerance for `exp` / `iat`. Default 30.
    #[serde(default)]
    pub clock_skew_secs: Option<u64>,
}

impl TokenLookupConfig {
    pub fn ticket_param(&self) -> &str {
        let param = self.ticket_param.trim();
        if param.is_empty() {
            "ticket"
        } else {
            param
        }
    }

    pub fn subject_path(&self) -> &str {
        self.subject_path.as_deref().unwrap_or("/sub")
    }

    pub fn nickname_path(&self) -> &str {
        self.nickname_path.trim()
    }

    pub fn audience_path(&self) -> &str {
        self.audience_path.trim()
    }

    /// Pointer to `exp`. Defaults to `/exp`: an empty default silently
    /// disabled expiry checks, so a portal ticket stayed valid forever.
    pub fn expires_at_path(&self) -> &str {
        let path = self.expires_at_path.trim();
        if path.is_empty() {
            "/exp"
        } else {
            path
        }
    }

    /// Pointer to `iat`. Defaults to `/iat`.
    pub fn issued_at_path(&self) -> &str {
        let path = self.issued_at_path.trim();
        if path.is_empty() {
            "/iat"
        } else {
            path
        }
    }

    pub fn ticket_id_path(&self) -> &str {
        self.ticket_id_path.trim()
    }

    pub fn clock_skew_secs(&self) -> u64 {
        self.clock_skew_secs.unwrap_or(30)
    }

    fn validate(&self, what: &str) -> Result<()> {
        let lookup = self.lookup.url_opt();
        match lookup {
            Some(url) => validate_http_url(&format!("{what}.lookup.url"), url)?,
            None if self.signing.secret.trim().is_empty() => bail!(
                "{what}: requires `lookup.url` (opaque ticket) or `signing.secret` (signed ticket)"
            ),
            None => {}
        }
        if lookup.is_some() && !self.signing.secret.trim().is_empty() {
            bail!("{what}: set either `lookup.url` or `signing.secret`, not both");
        }
        if self.signing.secret_prev.trim().len() > 0 && self.signing.secret.trim().is_empty() {
            bail!("{what}.signing.secret_prev requires `signing.secret`");
        }
        if self.ticket_param().is_empty() {
            bail!("{what}.ticket_param must not be empty");
        }
        for (name, path) in [
            ("subject_path", self.subject_path()),
            ("nickname_path", self.nickname_path()),
            ("issuer_path", self.issuer_path.trim()),
            ("audience_path", self.audience_path()),
            ("expires_at_path", self.expires_at_path()),
            ("issued_at_path", self.issued_at_path()),
            ("ticket_id_path", self.ticket_id_path()),
        ] {
            if name == "subject_path" && path.trim().is_empty() {
                bail!("{what}.subject_path must not be empty");
            }
            validate_optional_path(&format!("{what}.{name}"), Some(path))?;
        }
        Ok(())
    }
}

/// One `[auth.providers.<id>]` entry. The map key is the provider id.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthProviderConfig {
    pub kind: AuthProviderKind,
    /// Defaults to enabled. Disabled entries are skipped by the registry.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Human label for login pages.
    #[serde(default)]
    pub display_name: String,
    /// Marks the login page's primary entry (solid button, first row).
    ///
    /// Optional: when no provider sets it, the page falls back to the kind
    /// priority `platform > oidc > oauth2_userinfo > local_password`.
    #[serde(default)]
    pub primary: bool,
    /// Issuer recorded on [`crate::auth_provider::Identity`] when the response
    /// carries no issuer claim.
    #[serde(default)]
    pub issuer: String,
    /// `local_password` is standalone-only by default; a managed deployment
    /// must opt in explicitly because it bypasses the access allowlist.
    #[serde(default)]
    pub allow_in_managed: bool,
    /// Browser callback path for this provider. Empty → `/api/auth/callback/{id}`.
    /// `platform` keeps the legacy path by default so a control-plane allowlist
    /// does not have to change when the server moves to one callback route.
    #[serde(default)]
    pub callback_path: String,
    /// Login-button icon name (`building` / `key` / `ticket` / `user` / `shield`,
    /// or any name the SPA knows). Empty falls back to the per-kind default.
    #[serde(default)]
    pub icon: String,
    /// Browser authorize endpoint (required for `oauth2_userinfo`).
    #[serde(default)]
    pub authorize: AuthorizeEndpointConfig,
    #[serde(default)]
    pub token: TokenEndpointConfig,
    #[serde(default)]
    pub userinfo: UserinfoEndpointConfig,
    /// `kind = "oidc"` settings.
    #[serde(default)]
    pub oidc: OidcConfig,
    /// `kind = "token_lookup"` settings.
    #[serde(default)]
    pub token_lookup: TokenLookupConfig,
}

impl AuthProviderConfig {
    /// Minimal entry for a built-in provider injected by deployment mode.
    pub fn builtin(kind: AuthProviderKind, display_name: &str) -> Self {
        Self {
            kind,
            enabled: None,
            display_name: display_name.to_string(),
            primary: false,
            issuer: String::new(),
            allow_in_managed: false,
            callback_path: String::new(),
            icon: String::new(),
            authorize: AuthorizeEndpointConfig::default(),
            token: TokenEndpointConfig::default(),
            userinfo: UserinfoEndpointConfig::default(),
            oidc: OidcConfig::default(),
            token_lookup: TokenLookupConfig::default(),
        }
    }

    /// Explicitly configured callback path, when it is usable.
    pub fn callback_path_opt(&self) -> Option<&str> {
        let path = self.callback_path.trim();
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    }

    /// Icon name for the login button: explicit config wins, else per-kind.
    pub fn icon_name(&self) -> &str {
        let explicit = self.icon.trim();
        if !explicit.is_empty() {
            return explicit;
        }
        match self.kind {
            AuthProviderKind::Platform => "building",
            AuthProviderKind::LocalPassword => "key",
            AuthProviderKind::TokenLookup => "ticket",
            AuthProviderKind::Oauth2Userinfo => "user",
            AuthProviderKind::Oidc => "shield",
        }
    }

    /// Effective browser callback path for this provider id.
    pub fn callback_path(&self, id: &str) -> String {
        match self.callback_path_opt() {
            Some(path) => path.to_string(),
            None => default_callback_path(id),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// Issuer for [`crate::auth_provider::Identity`], `None` when unset.
    pub fn issuer_opt(&self) -> Option<String> {
        let issuer = self.issuer.trim();
        if issuer.is_empty() {
            None
        } else {
            Some(issuer.to_string())
        }
    }

    /// Validate one provider entry under its registry id. Only the sections the
    /// selected `kind` actually uses are validated, so an `oidc` entry does not
    /// need token/userinfo endpoints and vice versa.
    pub fn validate(&self, id: &str) -> Result<()> {
        validate_provider_id(id)?;
        if let Some(path) = self.callback_path_opt() {
            validate_callback_path(path)?;
        }
        match self.kind {
            AuthProviderKind::Platform | AuthProviderKind::LocalPassword => {}
            AuthProviderKind::TokenLookup => {
                self.token_lookup
                    .validate(&format!("auth provider `{id}`: token_lookup"))?;
            }
            AuthProviderKind::Oauth2Userinfo => {
                self.authorize
                    .validate(&format!("auth provider `{id}`: authorize"), true)?;
                self.token.validate(&format!("auth provider `{id}`: token"))?;
                self.userinfo
                    .validate(&format!("auth provider `{id}`: userinfo"))?;
            }
            AuthProviderKind::Oidc => {
                self.oidc.validate(
                    &format!("auth provider `{id}`: oidc"),
                    self.issuer.as_str(),
                )?;
                self.authorize
                    .validate(&format!("auth provider `{id}`: authorize"), false)?;
            }
        }
        Ok(())
    }
}

/// Callback path used when a provider does not configure one.
pub fn default_callback_path(id: &str) -> String {
    format!("/api/auth/callback/{id}")
}

/// Callback path the control plane is registered with. The `platform` provider
/// keeps it unless the config overrides `callback_path`, so an existing
/// allowlist keeps working.
pub const PLATFORM_CALLBACK_PATH: &str = "/api/auth/oauth/callback";

fn validate_callback_path(path: &str) -> Result<()> {
    if !path.starts_with('/') {
        bail!("callback_path `{path}` must start with `/`");
    }
    if path.len() > 128 {
        bail!("callback_path `{path}` is longer than 128 characters");
    }
    if path.contains('?') || path.contains('#') || path.chars().any(char::is_whitespace) {
        bail!("callback_path `{path}` must be a plain path (no query, fragment, or spaces)");
    }
    Ok(())
}

/// Provider ids are config keys and end up in URLs/logs: keep them boring.
fn validate_provider_id(id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("auth provider id must not be empty");
    }
    if id.len() > 64 {
        bail!("auth provider id `{id}` is longer than 64 characters");
    }
    if let Some(bad) = id
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-'))
    {
        bail!("auth provider id `{id}` contains `{bad}` (allowed: a-z 0-9 _ -)");
    }
    Ok(())
}

fn validate_http_url(what: &str, url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        bail!("{what} is required");
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        bail!("{what} `{url}` must be an absolute http(s) URL");
    }
    Ok(())
}

/// Empty means "not configured" for every optional pointer.
fn validate_optional_path(what: &str, path: Option<&str>) -> Result<()> {
    match path {
        Some(path) if !path.is_empty() => {
            json_path::validate(path).map_err(|e| anyhow::anyhow!("{what}: {e}"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: empty `expires_at_path` / `issued_at_path` used to mean
    /// "never check", so an expired portal ticket still logged in.
    #[test]
    fn ticket_time_paths_default_to_the_jwt_claims() {
        let cfg = TokenLookupConfig::default();
        assert_eq!(cfg.expires_at_path(), "/exp");
        assert_eq!(cfg.issued_at_path(), "/iat");

        let mut custom = TokenLookupConfig::default();
        custom.expires_at_path = "/claims/exp".to_string();
        custom.issued_at_path = "/claims/iat".to_string();
        assert_eq!(custom.expires_at_path(), "/claims/exp");
        assert_eq!(custom.issued_at_path(), "/claims/iat");
    }

    /// DingTalk as the reference provider: camelCase token body, flat userinfo
    /// with `unionId`, access token in a custom header.
    const DINGTALK_TOML: &str = r#"
kind = "oauth2_userinfo"
display_name = "钉钉"
issuer = "https://login.dingtalk.com"

[authorize]
url = "https://login.dingtalk.com/oauth2/auth"
scope = "openid"

[token]
url = "https://api.dingtalk.com/v1.0/oauth2/userAccessToken"
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
error = { kind = "path_present", path = "/code" }
error_code_path = "/code"
error_message_path = "/message"

[userinfo]
url = "https://api.dingtalk.com/v1.0/contact/users/me"
token_auth = "header"
token_name = "x-acs-dingtalk-access-token"
token_prefix = ""
subject_path = "/unionId"
error = { kind = "path_present", path = "/code" }
"#;

    fn parse(raw: &str) -> Result<AuthProviderConfig> {
        toml::from_str::<AuthProviderConfig>(raw).map_err(Into::into)
    }

    #[test]
    fn parses_dingtalk_provider() {
        let cfg = parse(DINGTALK_TOML).expect("dingtalk config parses");
        assert!(cfg.is_enabled());
        assert_eq!(cfg.kind, AuthProviderKind::Oauth2Userinfo);
        assert_eq!(cfg.display_name, "钉钉");
        assert_eq!(cfg.issuer_opt().as_deref(), Some("https://login.dingtalk.com"));

        let token = &cfg.token;
        assert_eq!(token.method(), HttpMethod::Post);
        assert_eq!(token.auth, ClientAuth::ClientSecretPost);
        assert_eq!(token.encoding, ParamEncoding::Json);
        assert_eq!(token.grant_type_param(), "grantType");
        assert_eq!(token.client_id_param(), "clientId");
        assert_eq!(token.client_secret_param(), "clientSecret");
        assert_eq!(token.access_token_path(), "/accessToken");
        // Explicitly empty → the parameter is omitted.
        assert_eq!(token.redirect_uri_param(), "");

        let userinfo = &cfg.userinfo;
        assert_eq!(userinfo.method(), HttpMethod::Get);
        assert_eq!(userinfo.token_auth, TokenAuth::Header);
        assert_eq!(userinfo.token_name(), "x-acs-dingtalk-access-token");
        assert_eq!(userinfo.token_prefix(), "");
        assert_eq!(userinfo.subject_path(), "/unionId");
        cfg.validate("dingtalk").expect("dingtalk config validates");
    }

    #[test]
    fn defaults_match_oauth2_norms() {
        let cfg = parse(
            r#"
kind = "oauth2_userinfo"
[authorize]
url = "https://idp.example.com/authorize"
[token]
url = "https://idp.example.com/token"
client_id = "id"
client_secret = "secret"
[userinfo]
url = "https://idp.example.com/userinfo"
"#,
        )
        .expect("minimal config parses");
        let token = &cfg.token;
        assert_eq!(token.grant_type_param(), "grant_type");
        assert_eq!(token.grant_type_value(), "authorization_code");
        assert_eq!(token.code_param(), "code");
        assert_eq!(token.redirect_uri_param(), "redirect_uri");
        assert_eq!(token.code_verifier_param(), "code_verifier");
        assert_eq!(token.access_token_path(), "/access_token");
        assert_eq!(token.error, ErrorRule::HttpStatus);

        let userinfo = &cfg.userinfo;
        assert_eq!(userinfo.token_auth, TokenAuth::Header);
        assert_eq!(userinfo.token_name(), "Authorization");
        assert_eq!(userinfo.token_prefix(), "Bearer ");
        assert_eq!(userinfo.subject_path(), "/sub");
        // PKCE is on unless the provider cannot do it.
        assert!(cfg.authorize.pkce());
        cfg.validate("idp").expect("minimal config validates");
    }

    #[test]
    fn oidc_config_defaults_and_discovery_url() {
        let cfg = parse(
            r#"
kind = "oidc"
display_name = "企业 SSO"
issuer = "https://idp.example.com"
[oidc]
client_id = "oidc-client"
client_secret = "oidc-secret"
scopes = ["profile email"]
"#,
        )
        .expect("oidc config parses");
        assert_eq!(cfg.kind, AuthProviderKind::Oidc);
        cfg.validate("corp").expect("oidc config validates");

        assert_eq!(
            cfg.oidc.discovery_url("https://idp.example.com/").as_deref(),
            Some("https://idp.example.com/.well-known/openid-configuration")
        );
        assert_eq!(cfg.oidc.audience(), "oidc-client");
        assert_eq!(cfg.oidc.subject_path(), "/sub");
        assert_eq!(cfg.oidc.algs(), vec!["RS256".to_string()]);
        assert_eq!(cfg.oidc.clock_skew_secs(), 120);
        assert_eq!(
            cfg.oidc.scopes(),
            vec!["openid".to_string(), "profile".to_string(), "email".to_string()]
        );
        // No authorize URL: the discovered `authorization_endpoint` is used.
        assert!(cfg.authorize.url_opt().is_none());

        let mut bad = cfg.clone();
        bad.issuer = String::new();
        assert!(bad
            .validate("corp")
            .unwrap_err()
            .to_string()
            .contains("`issuer` is required"));

        let mut bad = cfg.clone();
        bad.oidc.client_secret = String::new();
        assert!(bad
            .validate("corp")
            .unwrap_err()
            .to_string()
            .contains("client_secret"));

        let mut bad = cfg.clone();
        bad.oidc.algs = vec!["HS256".into()];
        assert!(bad
            .validate("corp")
            .unwrap_err()
            .to_string()
            .contains("HS256"));

        // `token` / `userinfo` are not required for oidc entries.
        let mut bad = cfg;
        bad.token.url = String::new();
        bad.userinfo.url = String::new();
        bad.validate("corp").expect("oidc ignores token/userinfo");
    }

    #[test]
    fn oauth2_userinfo_requires_an_authorize_endpoint() {
        let cfg = parse(DINGTALK_TOML).unwrap();
        let mut bad = cfg;
        bad.authorize.url = String::new();
        assert!(bad
            .validate("dingtalk")
            .unwrap_err()
            .to_string()
            .contains("authorize.url is required"));
    }

    #[test]
    fn unknown_kind_and_token_auth_are_rejected_at_parse_time() {
        let err = parse(
            r#"
kind = "saml"
[token]
url = "https://idp.example.com/token"
[userinfo]
url = "https://idp.example.com/userinfo"
"#,
        )
        .expect_err("saml is not a supported kind");
        assert!(err.to_string().contains("unknown variant"), "{err}");

        let err = parse(
            r#"
kind = "oauth2_userinfo"
[token]
url = "https://idp.example.com/token"
client_id = "id"
client_secret = "secret"
[userinfo]
url = "https://idp.example.com/userinfo"
token_auth = "cookie"
"#,
        )
        .expect_err("unknown token_auth");
        assert!(err.to_string().contains("unknown variant"), "{err}");
    }

    #[test]
    fn rejects_bad_ids_urls_paths_and_placements() {
        let cfg = parse(DINGTALK_TOML).unwrap();
        assert!(cfg.validate("DingTalk").is_err(), "uppercase id");
        assert!(cfg.validate("ding talk").is_err(), "space in id");
        assert!(cfg.validate("dingtalk").is_ok());

        let mut bad = cfg.clone();
        bad.token.url = "api.dingtalk.com/token".into();
        assert!(bad.validate("dingtalk").unwrap_err().to_string().contains("http(s)"));

        let mut bad = cfg.clone();
        bad.userinfo.subject_path = Some("unionId".into());
        assert!(bad
            .validate("dingtalk")
            .unwrap_err()
            .to_string()
            .contains("must start with `/`"));

        let mut bad = cfg.clone();
        bad.userinfo.subject_path = Some("/$.unionId".into());
        assert!(bad
            .validate("dingtalk")
            .unwrap_err()
            .to_string()
            .contains("contains `$`"));

        let mut bad = cfg.clone();
        bad.userinfo.subject_path = Some(String::new());
        assert!(bad.validate("dingtalk").unwrap_err().to_string().contains("subject_path"));

        let mut bad = cfg.clone();
        bad.userinfo.token_auth = TokenAuth::Body;
        assert!(bad
            .validate("dingtalk")
            .unwrap_err()
            .to_string()
            .contains("requires method"));

        let mut bad = cfg.clone();
        bad.token.auth = ClientAuth::ClientSecretBasic;
        bad.token.client_secret = String::new();
        assert!(bad.validate("dingtalk").unwrap_err().to_string().contains("client_secret"));

        let mut bad = cfg;
        bad.token.error = ErrorRule::PathNotEqual {
            path: "/errcode".into(),
            ok_value: String::new(),
        };
        assert!(bad.validate("dingtalk").unwrap_err().to_string().contains("ok_value"));
    }
}
