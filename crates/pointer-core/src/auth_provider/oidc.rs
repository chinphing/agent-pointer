//! `oidc` driver: discovery → authorization code → JWKS-verified ID token.
//!
//! Nothing is trusted before the signature checks out: the discovery document
//! is fetched from the configured issuer, the ID token is verified against the
//! published JWKS, and `iss` / `aud` / `exp` / `nonce` are all compared with
//! what this login attempt asked for. No expression engine, no scripts — the
//! only configurable pieces are endpoint overrides and restricted JSON pointers.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::config::{AuthProviderConfig, AuthProviderKind, ClientAuth, OidcConfig};
use super::identity::{AuthCodeRequest, AuthProviderDriver, AuthorizeRequest, Identity};
use super::json_path;
use super::query;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Discovery documents rarely change; one hour keeps startup and login cheap.
const METADATA_TTL: Duration = Duration::from_secs(3600);
/// Signing keys rotate on the order of weeks.
const JWKS_TTL: Duration = Duration::from_secs(3600);
/// Minimum gap between forced JWKS refetches (unknown `kid` / bad signature),
/// so a stream of forged tokens cannot turn into a request amplifier.
const JWKS_MIN_REFETCH: Duration = Duration::from_secs(30);
const BODY_SNIPPET_LIMIT: usize = 200;

/// The parts of the discovery document this driver consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct OidcMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

/// One RSA signing key from the provider JWKS.
#[derive(Debug, Clone)]
struct RsaJwk {
    kid: Option<String>,
    /// Big-endian modulus, no leading zeros.
    n: Vec<u8>,
    /// Big-endian public exponent.
    e: Vec<u8>,
    /// `alg` from the JWK, when the provider publishes one.
    alg: Option<String>,
}

#[derive(Default)]
struct Cache {
    metadata: HashMap<String, (OidcMetadata, Instant)>,
    jwks: HashMap<String, (Vec<RsaJwk>, Instant)>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Drop cached discovery documents and keys.
///
/// Tests spin up throwaway IdPs whose ports get reused, so a cached entry from
/// an earlier test can otherwise answer for a later one.
#[cfg(test)]
fn reset_caches() {
    let mut cache = cache().lock();
    cache.metadata.clear();
    cache.jwks.clear();
}

/// OpenID Connect driver. Stateless per login: `state`, PKCE verifiers, and
/// nonces live in the server layer.
pub struct OidcDriver {
    provider_id: String,
    config: AuthProviderConfig,
    http: reqwest::Client,
}

impl OidcDriver {
    pub fn new(provider_id: impl Into<String>, config: AuthProviderConfig) -> Result<Self> {
        let provider_id = provider_id.into();
        if config.kind != AuthProviderKind::Oidc {
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

    fn oidc(&self) -> &OidcConfig {
        &self.config.oidc
    }

    /// Discovery document, cached per URL.
    pub async fn metadata(&self) -> Result<OidcMetadata> {
        let url = self
            .oidc()
            .discovery_url(&self.config.issuer)
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: no issuer or discovery_url configured",
                    self.provider_id
                )
            })?;
        if let Some((metadata, at)) = cache().lock().metadata.get(&url).cloned() {
            if at.elapsed() < METADATA_TTL {
                return Ok(metadata);
            }
        }
        let body = self.fetch_json(&url, "discovery").await?;
        let metadata = parse_metadata(&self.provider_id, &body)?;
        // Mix-up defense: the document must describe the issuer we configured,
        // otherwise a hijacked discovery URL could point at another tenant.
        let configured = self.config.issuer.trim().trim_end_matches('/');
        if !configured.is_empty() && normalize_issuer(&metadata.issuer) != configured {
            bail!(
                "auth provider `{}`: discovery issuer `{}` does not match configured issuer `{configured}`",
                self.provider_id,
                metadata.issuer
            );
        }
        cache()
            .lock()
            .metadata
            .insert(url, (metadata.clone(), Instant::now()));
        Ok(metadata)
    }

    /// Browser redirect target: the discovered endpoint, or the configured
    /// override when the provider needs extra query parameters.
    pub async fn build_authorize_url(&self, request: &AuthorizeRequest<'_>) -> Result<String> {
        let metadata = self.metadata().await?;
        let cfg = &self.config.authorize;
        let base = cfg
            .url_opt()
            .map(str::to_string)
            .unwrap_or(metadata.authorization_endpoint);
        let mut params: Vec<(&str, String)> = vec![
            ("response_type", "code".to_string()),
            ("client_id", self.oidc().client_id.trim().to_string()),
            ("redirect_uri", request.redirect_uri.trim().to_string()),
            ("state", request.state.trim().to_string()),
            ("scope", self.oidc().scopes().join(" ")),
            (
                "nonce",
                request
                    .nonce
                    .map(|nonce| nonce.trim().to_string())
                    .unwrap_or_default(),
            ),
        ];
        if cfg.pkce() {
            if let Some(challenge) = request.code_challenge.filter(|c| !c.trim().is_empty()) {
                params.push(("code_challenge", challenge.trim().to_string()));
                params.push(("code_challenge_method", "S256".to_string()));
            }
        }
        for (name, value) in &cfg.extra_params {
            params.push((name.as_str(), value.clone()));
        }
        Ok(query::with_params(&base, &params))
    }

    /// Signing keys, cached per JWKS URL. `force` bypasses the TTL but never the
    /// refetch floor, so key rotation is picked up without hammering the IdP.
    async fn jwks(&self, jwks_uri: &str, force: bool) -> Result<Vec<RsaJwk>> {
        let cached = cache().lock().jwks.get(jwks_uri).cloned();
        if let Some((keys, at)) = cached {
            let fresh = at.elapsed() < JWKS_TTL;
            if fresh && !force {
                return Ok(keys);
            }
            if at.elapsed() < JWKS_MIN_REFETCH {
                return Ok(keys);
            }
        }
        let body = self.fetch_json(jwks_uri, "jwks").await?;
        let keys = parse_jwks(&self.provider_id, &body)?;
        cache()
            .lock()
            .jwks
            .insert(jwks_uri.to_string(), (keys.clone(), Instant::now()));
        Ok(keys)
    }

    /// Exchange the code for tokens and verify the returned ID token.
    pub async fn authenticate_code(&self, request: &AuthCodeRequest<'_>) -> Result<Identity> {
        let code = request.code.trim();
        if code.is_empty() {
            bail!(
                "auth provider `{}`: authorization code is empty",
                self.provider_id
            );
        }
        let metadata = self.metadata().await?;
        let mut params: Vec<(String, String)> = vec![
            ("grant_type".to_string(), "authorization_code".to_string()),
            ("code".to_string(), code.to_string()),
            (
                "redirect_uri".to_string(),
                request.redirect_uri.trim().to_string(),
            ),
        ];
        match self.oidc().auth {
            ClientAuth::ClientSecretPost => {
                params.push(("client_id".into(), self.oidc().client_id.trim().into()));
                params.push((
                    "client_secret".into(),
                    self.oidc().client_secret.trim().into(),
                ));
            }
            ClientAuth::ClientSecretBasic => {}
            ClientAuth::None => {
                params.push(("client_id".into(), self.oidc().client_id.trim().into()));
            }
        }
        if let Some(verifier) = request.code_verifier.filter(|v| !v.trim().is_empty()) {
            params.push(("code_verifier".into(), verifier.trim().into()));
        }

        let mut builder = self.http.post(&metadata.token_endpoint).form(&params);
        if self.oidc().auth == ClientAuth::ClientSecretBasic {
            builder = builder.basic_auth(
                self.oidc().client_id.trim(),
                Some(self.oidc().client_secret.trim()),
            );
        }
        let (status, text) = self.send(builder, "token").await?;
        let body = parse_success_body(&self.provider_id, "token", status, &text)?;
        let id_token = body
            .get("id_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: token response has no `id_token`",
                    self.provider_id
                )
            })?;

        self.verify_id_token(id_token, &metadata, request.nonce)
            .await
    }

    /// Full ID token verification: signature, then `iss` / `aud` / `exp` / `nonce`.
    async fn verify_id_token(
        &self,
        id_token: &str,
        metadata: &OidcMetadata,
        expected_nonce: Option<&str>,
    ) -> Result<Identity> {
        let (header, claims, signing_input, signature) =
            split_jwt(&self.provider_id, id_token)?;

        let alg = header
            .get("alg")
            .and_then(Value::as_str)
            .map(|alg| alg.trim().to_ascii_uppercase())
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: id_token has no `alg`",
                    self.provider_id
                )
            })?;
        if !self.oidc().algs().iter().any(|allowed| allowed == &alg) {
            bail!(
                "auth provider `{}`: id_token alg `{alg}` is not accepted",
                self.provider_id
            );
        }
        let kid = header
            .get("kid")
            .and_then(Value::as_str)
            .map(str::to_string);

        let keys = self.jwks(&metadata.jwks_uri, false).await?;
        if !verify_with_keys(&keys, kid.as_deref(), &alg, &signing_input, &signature) {
            // Rotation or a stale cache: refetch once, then give up.
            let keys = self.jwks(&metadata.jwks_uri, true).await?;
            if !verify_with_keys(&keys, kid.as_deref(), &alg, &signing_input, &signature) {
                bail!(
                    "auth provider `{}`: id_token signature is not valid for any published key",
                    self.provider_id
                );
            }
        }

        // Signature is good — only now are the claims worth reading.
        let issuer = claims
            .get("iss")
            .and_then(Value::as_str)
            .map(normalize_issuer)
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: id_token has no `iss`",
                    self.provider_id
                )
            })?;
        if issuer != normalize_issuer(&metadata.issuer) {
            bail!(
                "auth provider `{}`: id_token issuer `{issuer}` does not match `{}`",
                self.provider_id,
                metadata.issuer
            );
        }

        let audience = self.oidc().audience();
        if !audience_matches(&claims, &audience) {
            bail!(
                "auth provider `{}`: id_token audience does not include `{audience}`",
                self.provider_id
            );
        }

        let skew = self.oidc().clock_skew_secs() as i64;
        let now = chrono::Utc::now().timestamp();
        let exp = claims
            .get("exp")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: id_token has no `exp`",
                    self.provider_id
                )
            })?;
        if now - skew >= exp {
            bail!(
                "auth provider `{}`: id_token expired (exp={exp}, now={now})",
                self.provider_id
            );
        }
        if let Some(iat) = claims.get("iat").and_then(Value::as_i64) {
            if iat - skew > now {
                bail!(
                    "auth provider `{}`: id_token was issued in the future (iat={iat})",
                    self.provider_id
                );
            }
        }

        // `nonce` binds the token to the browser attempt that started the flow.
        // A provider that returns no nonce when one was requested is rejected.
        if let Some(expected) = expected_nonce.map(str::trim).filter(|n| !n.is_empty()) {
            match claims.get("nonce").and_then(Value::as_str) {
                Some(actual) if actual.trim() == expected => {}
                Some(_) => bail!(
                    "auth provider `{}`: id_token nonce does not match this login attempt",
                    self.provider_id
                ),
                None => bail!(
                    "auth provider `{}`: id_token has no `nonce` but one was requested",
                    self.provider_id
                ),
            }
        }

        let subject_path = self.oidc().subject_path();
        let subject = json_path::as_string(&claims, subject_path)
            .map(|subject| subject.trim().to_string())
            .filter(|subject| !subject.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: id_token has no subject at `{subject_path}`",
                    self.provider_id
                )
            })?;

        Ok(Identity {
            provider_id: self.provider_id.clone(),
            issuer: Some(issuer),
            subject,
            claims,
        })
    }

    async fn fetch_json(&self, url: &str, stage: &str) -> Result<Value> {
        if !is_http_url(url) {
            bail!(
                "auth provider `{}`: {stage} URL `{url}` is not an absolute http(s) URL",
                self.provider_id
            );
        }
        let (status, text) = self.send(self.http.get(url), stage).await?;
        parse_success_body(&self.provider_id, stage, status, &text)
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
impl AuthProviderDriver for OidcDriver {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn kind(&self) -> AuthProviderKind {
        AuthProviderKind::Oidc
    }

    async fn authorize_url(&self, request: &AuthorizeRequest<'_>) -> Result<Option<String>> {
        self.build_authorize_url(request).await.map(Some)
    }

    async fn authenticate(&self, request: AuthCodeRequest<'_>) -> Result<Identity> {
        self.authenticate_code(&request).await
    }
}

fn parse_metadata(provider_id: &str, body: &Value) -> Result<OidcMetadata> {
    let field = |name: &str| -> Result<String> {
        body.get(name)
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                anyhow!("auth provider `{provider_id}`: discovery document has no `{name}`")
            })
    };
    let metadata = OidcMetadata {
        issuer: field("issuer")?,
        authorization_endpoint: field("authorization_endpoint")?,
        token_endpoint: field("token_endpoint")?,
        jwks_uri: field("jwks_uri")?,
    };
    for (name, url) in [
        ("issuer", &metadata.issuer),
        ("authorization_endpoint", &metadata.authorization_endpoint),
        ("token_endpoint", &metadata.token_endpoint),
        ("jwks_uri", &metadata.jwks_uri),
    ] {
        if !is_http_url(url) {
            bail!("auth provider `{provider_id}`: discovery `{name}` `{url}` is not an http(s) URL");
        }
    }
    Ok(metadata)
}

fn parse_jwks(provider_id: &str, body: &Value) -> Result<Vec<RsaJwk>> {
    let keys = body
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("auth provider `{provider_id}`: JWKS has no `keys` array"))?;
    let mut out = Vec::new();
    for key in keys {
        // Only RSA signing keys are usable here; anything else is skipped
        // rather than treated as a failure (providers publish encryption keys too).
        if key.get("kty").and_then(Value::as_str) != Some("RSA") {
            continue;
        }
        if let Some(use_) = key.get("use").and_then(Value::as_str) {
            if use_ != "sig" {
                continue;
            }
        }
        let (Some(n), Some(e)) = (
            key.get("n").and_then(Value::as_str),
            key.get("e").and_then(Value::as_str),
        ) else {
            continue;
        };
        let (Some(n), Some(e)) = (decode_b64url(n), decode_b64url(e)) else {
            continue;
        };
        out.push(RsaJwk {
            kid: key
                .get("kid")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|kid| !kid.is_empty()),
            n,
            e,
            alg: key
                .get("alg")
                .and_then(Value::as_str)
                .map(|alg| alg.trim().to_ascii_uppercase()),
        });
    }
    if out.is_empty() {
        bail!("auth provider `{provider_id}`: JWKS has no usable RSA signing key");
    }
    Ok(out)
}

fn verify_with_keys(
    keys: &[RsaJwk],
    kid: Option<&str>,
    alg: &str,
    signing_input: &[u8],
    signature: &[u8],
) -> bool {
    let candidates: Vec<&RsaJwk> = match kid {
        Some(kid) => keys
            .iter()
            .filter(|key| key.kid.as_deref() == Some(kid))
            .collect(),
        // No `kid` on the token: the only safe case is a single published key.
        None if keys.len() == 1 => keys.iter().collect(),
        None => Vec::new(),
    };
    candidates.into_iter().any(|key| {
        if let Some(key_alg) = &key.alg {
            if key_alg != alg {
                return false;
            }
        }
        verify_rsa(key, alg, signing_input, signature)
    })
}

fn verify_rsa(key: &RsaJwk, alg: &str, message: &[u8], signature: &[u8]) -> bool {
    let params = match alg {
        "RS256" => &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        "RS384" => &ring::signature::RSA_PKCS1_2048_8192_SHA384,
        "RS512" => &ring::signature::RSA_PKCS1_2048_8192_SHA512,
        _ => return false,
    };
    ring::signature::RsaPublicKeyComponents {
        n: key.n.as_slice(),
        e: key.e.as_slice(),
    }
    .verify(params, message, signature)
    .is_ok()
}

/// Split a compact JWT into `(header, claims, signing input, signature)`.
fn split_jwt(provider_id: &str, token: &str) -> Result<(Value, Value, Vec<u8>, Vec<u8>)> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 {
        bail!("auth provider `{provider_id}`: id_token is not a compact JWS");
    }
    let header_bytes = decode_b64url(parts[0])
        .ok_or_else(|| anyhow!("auth provider `{provider_id}`: id_token header is not base64url"))?;
    let header: Value = serde_json::from_slice(&header_bytes)
        .map_err(|e| anyhow!("auth provider `{provider_id}`: id_token header is not JSON: {e}"))?;
    let claims_bytes = decode_b64url(parts[1]).ok_or_else(|| {
        anyhow!("auth provider `{provider_id}`: id_token claims are not base64url")
    })?;
    let claims: Value = serde_json::from_slice(&claims_bytes)
        .map_err(|e| anyhow!("auth provider `{provider_id}`: id_token claims are not JSON: {e}"))?;
    let signature = decode_b64url(parts[2])
        .ok_or_else(|| anyhow!("auth provider `{provider_id}`: id_token signature is not base64url"))?;
    let signing_input = format!("{}.{}", parts[0], parts[1]).into_bytes();
    Ok((header, claims, signing_input, signature))
}

fn audience_matches(claims: &Value, expected: &str) -> bool {
    match claims.get("aud") {
        Some(Value::String(aud)) => aud == expected,
        Some(Value::Array(items)) => items
            .iter()
            .any(|item| item.as_str().map(|aud| aud == expected).unwrap_or(false)),
        _ => false,
    }
}

fn parse_success_body(
    provider_id: &str,
    stage: &str,
    status: reqwest::StatusCode,
    text: &str,
) -> Result<Value> {
    if !status.is_success() {
        bail!(
            "auth provider `{provider_id}`: {stage} request failed (HTTP {}): {}",
            status.as_u16(),
            snippet(text)
        );
    }
    let body: Value = serde_json::from_str(text).map_err(|_| {
        anyhow!(
            "auth provider `{provider_id}`: {stage} response is not JSON: {}",
            snippet(text)
        )
    })?;
    // RFC 6749 error responses can arrive with a 200 status from proxies.
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        bail!(
            "auth provider `{provider_id}`: {stage} request failed ({error}): {}",
            snippet(
                body.get("error_description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            )
        );
    }
    Ok(body)
}

fn decode_b64url(value: &str) -> Option<Vec<u8>> {
    let trimmed = value.trim().trim_end_matches('=');
    URL_SAFE_NO_PAD.decode(trimmed).ok()
}

fn normalize_issuer(issuer: &str) -> String {
    issuer.trim().trim_end_matches('/').to_string()
}

fn is_http_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
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
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Test-only RSA key (PKCS#8 DER, base64). Generated once for this module;
    /// it never leaves the test build.
    const TEST_KEY_PKCS8_B64: &str = "MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQDFoVZIeAVqRzDU6IwbG4KOtlsPSUPYH6NPk0rafKQOP+93Xo6iRKxyy/FmM1vCeMRC5Na09iAReOVW7kOs0duHOXI8jydP9xGfqP/rQs4xMBRoWcOxekVFPhyQayZyqByDVRdcpASlJ47RV9FgQQFvQmMQjVl/zRhrHb51tTvILuZ13pjYAoXruclbXzf22QYptroE5PTEVGAArOT476CSg9O+Po+Qq80Uy31Afvvu/mhLeiQHS1wxAGFaVLSR5IpHBG2mrbgZ8mD7vdbErPWnCHk+wU8TO384vDhVOwm6RVho9ISM3imSQnzfxfTwb7X/vP88O50VKMOfMwYUYIjhAgMBAAECggEADtJJuUefCFhsdhs2nrXSdceBTsUs3GGLCuW+AGLGnvb2i5pA0jSaRkW7RfHmGNCbz3SAEbQ/tW++q2VVF/npH9wUECygQyQBY1r2TnuFITU84pKQ1ZeY6Fd/JumSSZkIAmV4Sg9W9dfgoxyV7EUTyXdOHKitJKOKCEaDySodYKbZ///AXZs2o3wHJe7qzIU0/R7IP3gD9ZK7boExwZ2fXZlLENvnKr6jFjd67gJiKIVI0Rd0z28k6WUJL8J9c5oTblnA4U/eK63JmAlxpeVO9m+eHOy6VCLvQ795q79AwEW8VrtqsFAVCUx4tbMxTeR1RC76nL5WjqsXrGKj97qbzQKBgQDuhLBJLyT0UeEwwXeKfYL/+bmpre2rWfgr+OKiL0qxIKCJF5PjQU892ZLXtvtGaqwbKEC6Rq2QGYLJT8YnCzrDHRL/3Ml5HUJm58f4qX6SYfuPObpGUa5HnFGcXTnHS1lPePmng4LUnUBbxET9+Brj4UrlV36JLXGz0Syx0jiDNwKBgQDUHXdrjw2UKzvj3d+DoGZaPIA/dsbF3nmG8Cmq9+2zHk74PnIAtPA7BEsucb9JPHj7tVygwqZs51B1J5XVc4U2WBTIafabaQb/WQYVJI8Q1O6tDSk9u+ml5HbT2Enx4b69pkqwQdRO5TMxgWNT8CqSOo8fldTqzzN54MJW1+yQpwKBgENSjHd7qRCU89zRHq6mUo1xrSfpgcX1tQikTUO92XU5YlaPoxxn2Lf6a2obqbzsavuXVv286UxdBv9PVvy6PLHJyQ0TAoL+8TkefuD+5kzm/xwkDSG/+FMaGibqYV6RM/osiTrw8cKVSA8ezOJqXxynAH7PPVX2z7sTdWwTjAPtAoGAbgBWBRar4RJGvdY7auSND/DeRYtQbB8DASKdSUfIIqLuKzkfIBS1nLWOU6izncRZrZeRUOGBM2sGQ6tivc5p4BPvuzuQo2nlMhk12VMbSoxStrXjlFtWQSzpHtXFC0aaB3oss5mAA+ZDFUJXg5lCSpFJ1gR65cbMmEEiJ61UxnMCgYAD25OOmLtQBiUNC2uz0ckQa6Gr7IUEYyj/TsKVryrzs4If0TcJrYIG/vSHRZ8fa4jsOGhLtcABDoEpPJWyaDhrl8In6rV2cmsT5mwIwPd5PnKpN78t0jwUd2QoKpUuA/hYBhDbwteBmWU8LsgGCr041sya/33TvKugYFc2i49x+g==";
    /// Modulus of [`TEST_KEY_PKCS8_B64`], uppercase hex.
    const TEST_MODULUS_HEX: &str = "C5A1564878056A4730D4E88C1B1B828EB65B0F4943D81FA34F934ADA7CA40E3FEF775E8EA244AC72CBF166335BC278C442E4D6B4F6201178E556EE43ACD1DB8739723C8F274FF7119FA8FFEB42CE3130146859C3B17A45453E1C906B2672A81C8355175CA404A5278ED157D16041016F4263108D597FCD186B1DBE75B53BC82EE675DE98D80285EBB9C95B5F37F6D90629B6BA04E4F4C4546000ACE4F8EFA09283D3BE3E8F90ABCD14CB7D407EFBEEFE684B7A24074B5C3100615A54B491E48A47046DA6ADB819F260FBBDD6C4ACF5A708793EC14F133B7F38BC38553B09BA455868F4848CDE2992427CDFC5F4F06FB5FFBCFF3C3B9D1528C39F3306146088E1";

    fn b64url(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(bytes)
    }

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        let bytes = hex.as_bytes();
        (0..bytes.len() / 2)
            .map(|i| {
                let hi = (bytes[i * 2] as char).to_digit(16).expect("hex") as u8;
                let lo = (bytes[i * 2 + 1] as char).to_digit(16).expect("hex") as u8;
                (hi << 4) | lo
            })
            .collect()
    }

    fn test_key() -> ring::signature::RsaKeyPair {
        let der = base64::engine::general_purpose::STANDARD
            .decode(TEST_KEY_PKCS8_B64)
            .expect("test key base64");
        ring::signature::RsaKeyPair::from_pkcs8(&der).expect("test key parses")
    }

    fn jwks_body(keys: &[(&str, &[u8])]) -> Value {
        let keys: Vec<Value> = keys
            .iter()
            .map(|(kid, modulus)| {
                json!({
                    "kty": "RSA",
                    "kid": kid,
                    "use": "sig",
                    "alg": "RS256",
                    "n": b64url(modulus),
                    "e": "AQAB",
                })
            })
            .collect();
        json!({ "keys": keys })
    }

    fn real_jwks() -> Value {
        jwks_body(&[("key-1", &hex_to_bytes(TEST_MODULUS_HEX))])
    }

    fn sign(key: &ring::signature::RsaKeyPair, header: Value, claims: &Value) -> String {
        let header = b64url(header.to_string().as_bytes());
        let payload = b64url(claims.to_string().as_bytes());
        let signing_input = format!("{header}.{payload}");
        let mut signature = vec![0u8; key.public().modulus_len()];
        key.sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            signing_input.as_bytes(),
            &mut signature,
        )
        .expect("sign");
        format!("{signing_input}.{}", b64url(&signature))
    }

    fn rs256_header() -> Value {
        json!({ "alg": "RS256", "typ": "JWT", "kid": "key-1" })
    }

    fn claims_for(issuer: &str, nonce: &str) -> Value {
        let now = chrono::Utc::now().timestamp();
        json!({
            "iss": issuer,
            "aud": "pointer-web",
            "sub": "user-42",
            "name": "张三",
            "iat": now,
            "exp": now + 300,
            "nonce": nonce,
        })
    }

    fn config_for(base: &str) -> AuthProviderConfig {
        toml::from_str(&format!(
            r#"
kind = "oidc"
display_name = "企业 SSO"
issuer = "{base}"

[oidc]
client_id = "pointer-web"
client_secret = "s3cret"
scopes = ["profile"]
"#
        ))
        .expect("oidc config parses")
    }

    async fn mount_discovery(server: &MockServer, issuer: &str) {
        reset_caches();
        let base = server.uri();
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": format!("{base}/token"),
                "jwks_uri": format!("{base}/jwks"),
                "id_token_signing_alg_values_supported": ["RS256"],
            })))
            .mount(server)
            .await;
    }

    async fn mount_jwks(server: &MockServer, body: Value) {
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn mount_token(server: &MockServer, id_token: &str) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=auth-code-1"))
            .and(body_string_contains("client_id=pointer-web"))
            .and(body_string_contains("code_verifier=verifier-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id_token": id_token })))
            .mount(server)
            .await;
    }

    fn login_request<'a>(nonce: Option<&'a str>) -> AuthCodeRequest<'a> {
        AuthCodeRequest {
            code: "auth-code-1",
            redirect_uri: "https://pointer.example.com/api/auth/callback/corp",
            code_verifier: Some("verifier-1"),
            nonce,
        }
    }

    #[tokio::test]
    async fn login_verifies_signature_and_returns_identity() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let token = sign(&test_key(), rs256_header(), &claims_for(&issuer, "nonce-1"));
        mount_token(&server, &token).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let identity = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect("login succeeds");

        assert_eq!(identity.provider_id, "corp");
        assert_eq!(identity.subject, "user-42");
        assert_eq!(identity.issuer.as_deref(), Some(issuer.as_str()));
        assert_eq!(identity.claims["name"], json!("张三"));
    }

    #[tokio::test]
    async fn authorize_url_uses_discovered_endpoint_and_nonce() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");

        let url = driver
            .authorize_url(&AuthorizeRequest {
                redirect_uri: "https://pointer.example.com/api/auth/callback/corp",
                state: "state-1",
                code_challenge: Some("challenge-1"),
                nonce: Some("nonce-1"),
            })
            .await
            .expect("authorize url builds")
            .expect("provider has an authorize url");

        assert!(url.starts_with(&format!("{issuer}/authorize?")), "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("client_id=pointer-web"), "{url}");
        assert!(url.contains("scope=openid%20profile"), "{url}");
        assert!(url.contains("state=state-1"), "{url}");
        assert!(url.contains("nonce=nonce-1"), "{url}");
        assert!(url.contains("code_challenge=challenge-1"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
    }

    #[tokio::test]
    async fn rejects_tampered_payload() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let key = test_key();
        let signed = sign(&key, rs256_header(), &claims_for(&issuer, "nonce-1"));
        // Swap the payload for a different subject, keeping the old signature.
        let mut tampered = claims_for(&issuer, "nonce-1");
        tampered["sub"] = json!("attacker");
        let parts: Vec<&str> = signed.split('.').collect();
        let forged = format!(
            "{}.{}.{}",
            parts[0],
            b64url(tampered.to_string().as_bytes()),
            parts[2]
        );
        mount_token(&server, &forged).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("tampered token is rejected");
        assert!(err.to_string().contains("signature"), "{err}");
    }

    #[tokio::test]
    async fn rejects_wrong_issuer_and_audience() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let mut claims = claims_for(&issuer, "nonce-1");
        claims["iss"] = json!("https://evil.example.com");
        let token = sign(&test_key(), rs256_header(), &claims);
        mount_token(&server, &token).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("issuer mismatch is rejected");
        assert!(err.to_string().contains("issuer"), "{err}");

        // Fresh server: the JWKS/metadata caches are keyed by URL.
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let mut claims = claims_for(&issuer, "nonce-1");
        claims["aud"] = json!("another-client");
        let token = sign(&test_key(), rs256_header(), &claims);
        mount_token(&server, &token).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("audience mismatch is rejected");
        assert!(err.to_string().contains("audience"), "{err}");
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let mut claims = claims_for(&issuer, "nonce-1");
        let past = chrono::Utc::now().timestamp() - 3600;
        claims["exp"] = json!(past);
        claims["iat"] = json!(past - 60);
        let token = sign(&test_key(), rs256_header(), &claims);
        mount_token(&server, &token).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("expired token is rejected");
        assert!(err.to_string().contains("expired"), "{err}");
    }

    #[tokio::test]
    async fn rejects_nonce_mismatch_and_missing_nonce() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let token = sign(&test_key(), rs256_header(), &claims_for(&issuer, "other-nonce"));
        mount_token(&server, &token).await;
        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("nonce mismatch is rejected");
        assert!(err.to_string().contains("nonce"), "{err}");

        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        mount_jwks(&server, real_jwks()).await;
        let mut claims = claims_for(&issuer, "");
        claims.as_object_mut().expect("object").remove("nonce");
        let token = sign(&test_key(), rs256_header(), &claims);
        mount_token(&server, &token).await;
        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("missing nonce is rejected");
        assert!(err.to_string().contains("nonce"), "{err}");
    }

    #[tokio::test]
    async fn rejects_alg_none() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        let claims = claims_for(&issuer, "nonce-1");
        let unsigned = format!(
            "{}.{}.",
            b64url(json!({"alg": "none"}).to_string().as_bytes()),
            b64url(claims.to_string().as_bytes())
        );
        mount_token(&server, &unsigned).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("alg none is rejected");
        assert!(err.to_string().contains("not accepted"), "{err}");
    }

    #[tokio::test]
    async fn rejects_discovery_issuer_mismatch() {
        let server = MockServer::start().await;
        // Discovery claims a different issuer than the configured one.
        mount_discovery(&server, "https://other-tenant.example.com").await;
        let driver = OidcDriver::new("corp", config_for(&server.uri())).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("issuer mix-up is rejected");
        assert!(
            err.to_string().contains("does not match configured issuer"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn accepts_rotated_key_selected_by_kid() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        // A decoy key first: selection must go by `kid`, not by position.
        let decoy = vec![0x11u8; 256];
        mount_jwks(
            &server,
            jwks_body(&[
                ("key-old", &decoy),
                ("key-1", &hex_to_bytes(TEST_MODULUS_HEX)),
            ]),
        )
        .await;
        let token = sign(&test_key(), rs256_header(), &claims_for(&issuer, "nonce-1"));
        mount_token(&server, &token).await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let identity = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect("login succeeds with the rotated key set");
        assert_eq!(identity.subject, "user-42");
    }

    #[tokio::test]
    async fn token_error_response_is_reported() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        mount_discovery(&server, &issuer).await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_grant",
                "error_description": "code expired"
            })))
            .mount(&server)
            .await;

        let driver = OidcDriver::new("corp", config_for(&issuer)).expect("driver builds");
        let err = driver
            .authenticate(login_request(Some("nonce-1")))
            .await
            .expect_err("token error is surfaced");
        assert!(err.to_string().contains("HTTP 400"), "{err}");
    }
}
