//! `token_lookup` driver: the portal hands the browser a ticket.
//!
//! Two verification modes, chosen by config:
//!
//! 1. **Signed ticket** (`signing.secret`): compact JWS with `HS256`, claims
//!    `sub` / `aud` / `iat` / `exp` / `jti` / `name`.
//! 2. **Opaque ticket** (`lookup.url`): the server calls the IdP and maps the
//!    JSON response through restricted pointers.
//!
//! Either way every redeemed ticket is recorded once per provider, so a replay
//! cannot mint a second session.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

use super::config::{AuthProviderConfig, AuthProviderKind, HttpMethod, ParamEncoding};
use super::identity::{AuthCodeRequest, AuthProviderDriver, AuthorizeRequest, Identity};
use super::json_path;

type HmacSha256 = Hmac<Sha256>;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const BODY_SNIPPET_LIMIT: usize = 200;
/// Floor for how long a redeemed ticket id stays blocked.
const MIN_REPLAY_RETENTION: Duration = Duration::from_secs(60);
/// Fixed header for minted tickets; `HS256` only, no `none` downgrade.
const TICKET_HEADER_JSON: &str = r#"{"alg":"HS256","typ":"SSO"}"#;

#[derive(Default)]
struct ReplayTable {
    seen: HashMap<String, Instant>,
}

fn replay_table() -> &'static Mutex<ReplayTable> {
    static TABLE: OnceLock<Mutex<ReplayTable>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(ReplayTable::default()))
}

/// Mark `ticket_id` as used for this provider. `false` means "already redeemed".
fn consume_once(provider_id: &str, ticket_id: &str, retain: Duration) -> bool {
    let key = format!("{provider_id}:{ticket_id}");
    let now = Instant::now();
    let mut table = replay_table().lock();
    table.seen.retain(|_, until| *until > now);
    if table.seen.contains_key(&key) {
        return false;
    }
    table
        .seen
        .insert(key, now + retain.max(MIN_REPLAY_RETENTION));
    true
}

/// Mint a short-lived signed ticket. Used by `pointer-server --mint-sso-ticket`
/// and by tests; the wire format matches what the driver verifies.
pub fn mint_signed_ticket(
    secret: &str,
    audience: &str,
    subject: &str,
    nickname: Option<&str>,
    ttl_secs: i64,
    now_unix: i64,
) -> Result<String> {
    let secret = secret.trim();
    let audience = audience.trim();
    let subject = subject.trim();
    if secret.is_empty() || audience.is_empty() || subject.is_empty() {
        bail!("ticket mint requires non-empty secret, audience, and subject");
    }
    let ttl = if ttl_secs > 0 { ttl_secs } else { 120 };
    let mut claims = json!({
        "sub": subject,
        "aud": audience,
        "iat": now_unix,
        "exp": now_unix + ttl,
        "jti": uuid::Uuid::new_v4().to_string(),
    });
    if let Some(nickname) = nickname.map(str::trim).filter(|name| !name.is_empty()) {
        claims["name"] = json!(nickname);
    }
    let header = b64url_encode(TICKET_HEADER_JSON.as_bytes());
    let payload = b64url_encode(
        serde_json::to_string(&claims)
            .context("serialize ticket claims")?
            .as_bytes(),
    );
    let signing_input = format!("{header}.{payload}");
    let signature = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
    Ok(format!("{signing_input}.{}", b64url_encode(&signature)))
}

/// Inbound-ticket provider. Not a redirect flow we start: the portal owns that.
pub struct TokenLookupDriver {
    provider_id: String,
    config: AuthProviderConfig,
    http: reqwest::Client,
}

impl TokenLookupDriver {
    pub fn new(provider_id: impl Into<String>, config: AuthProviderConfig) -> Result<Self> {
        let provider_id = provider_id.into();
        if config.kind != AuthProviderKind::TokenLookup {
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

    /// Query parameter the portal uses to hand us the ticket.
    pub fn ticket_param(&self) -> &str {
        self.config.token_lookup.ticket_param()
    }

    /// Verify one ticket and return the identity it stands for.
    pub async fn verify(&self, ticket: &str) -> Result<Identity> {
        let ticket = ticket.trim();
        if ticket.is_empty() {
            bail!("auth provider `{}`: ticket is empty", self.provider_id);
        }
        let claims = match self.config.token_lookup.lookup.url_opt() {
            Some(_) => self.lookup(ticket).await?,
            None => self.verify_signed(ticket)?,
        };
        self.identity_from_claims(claims, ticket)
    }

    /// Compact JWS check: `HS256` only, primary then previous secret.
    fn verify_signed(&self, ticket: &str) -> Result<Value> {
        let signing = &self.config.token_lookup.signing;
        let parts: Vec<&str> = ticket.split('.').collect();
        if parts.len() != 3 {
            bail!(
                "auth provider `{}`: ticket is not a compact JWS",
                self.provider_id
            );
        }
        let header_bytes = decode_b64url(parts[0]).ok_or_else(|| {
            anyhow!(
                "auth provider `{}`: ticket header is not base64url",
                self.provider_id
            )
        })?;
        let header: Value = serde_json::from_slice(&header_bytes).map_err(|e| {
            anyhow!(
                "auth provider `{}`: ticket header is not JSON: {e}",
                self.provider_id
            )
        })?;
        let alg = header
            .get("alg")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase();
        if alg != "HS256" {
            bail!(
                "auth provider `{}`: ticket alg `{alg}` is not accepted",
                self.provider_id
            );
        }
        let signature = decode_b64url(parts[2]).ok_or_else(|| {
            anyhow!(
                "auth provider `{}`: ticket signature is not base64url",
                self.provider_id
            )
        })?;
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let verified = [signing.secret.trim(), signing.secret_prev.trim()]
            .into_iter()
            .filter(|secret| !secret.is_empty())
            .any(|secret| {
                let expected = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
                constant_time_eq(&signature, &expected)
            });
        if !verified {
            bail!(
                "auth provider `{}`: ticket signature is not valid",
                self.provider_id
            );
        }
        let payload = decode_b64url(parts[1]).ok_or_else(|| {
            anyhow!(
                "auth provider `{}`: ticket claims are not base64url",
                self.provider_id
            )
        })?;
        serde_json::from_slice(&payload).map_err(|e| {
            anyhow!(
                "auth provider `{}`: ticket claims are not JSON: {e}",
                self.provider_id
            )
        })
    }

    /// Server-to-server verification of an opaque ticket.
    async fn lookup(&self, ticket: &str) -> Result<Value> {
        let endpoint = &self.config.token_lookup.lookup;
        let url = endpoint
            .url_opt()
            .ok_or_else(|| anyhow!("auth provider `{}`: lookup.url is required", self.provider_id))?;
        let name = endpoint.param();
        let mut builder = match endpoint.method() {
            HttpMethod::Get => self.http.get(url),
            HttpMethod::Post => self.http.post(url),
        };
        if !endpoint.token.trim().is_empty() {
            builder = builder.bearer_auth(endpoint.token.trim());
        }
        builder = match endpoint.method() {
            HttpMethod::Get => builder.query(&[(name, ticket)]),
            HttpMethod::Post => match endpoint.body_encoding() {
                ParamEncoding::Json => builder.json(&json!({ name: ticket })),
                ParamEncoding::Form => builder.form(&[(name, ticket)]),
            },
        };
        let response = builder.send().await.with_context(|| {
            format!(
                "auth provider `{}`: ticket lookup could not be sent",
                self.provider_id
            )
        })?;
        let status = response.status();
        let text = response.text().await.with_context(|| {
            format!(
                "auth provider `{}`: ticket lookup response could not be read",
                self.provider_id
            )
        })?;
        if !status.is_success() {
            bail!(
                "auth provider `{}`: ticket lookup failed (HTTP {}): {}",
                self.provider_id,
                status.as_u16(),
                snippet(&text)
            );
        }
        serde_json::from_str(&text).map_err(|_| {
            anyhow!(
                "auth provider `{}`: ticket lookup response is not JSON: {}",
                self.provider_id,
                snippet(&text)
            )
        })
    }

    /// Map the verified claims onto [`Identity`], including time and replay checks.
    fn identity_from_claims(&self, claims: Value, raw_ticket: &str) -> Result<Identity> {
        let cfg = &self.config.token_lookup;
        let now = chrono::Utc::now().timestamp();
        let skew = cfg.clock_skew_secs() as i64;

        let subject = json_path::as_string(&claims, cfg.subject_path())
            .map(|subject| subject.trim().to_string())
            .filter(|subject| !subject.is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "auth provider `{}`: ticket has no subject at `{}`",
                    self.provider_id,
                    cfg.subject_path()
                )
            })?;

        if let Some(exp) = path_i64(&claims, cfg.expires_at_path()) {
            if exp + skew < now {
                bail!("auth provider `{}`: ticket expired", self.provider_id);
            }
        }
        if let Some(iat) = path_i64(&claims, cfg.issued_at_path()) {
            if iat > now + skew {
                bail!(
                    "auth provider `{}`: ticket was issued in the future",
                    self.provider_id
                );
            }
        }
        let expected_audience = cfg.signing.audience.trim();
        if !expected_audience.is_empty() && !cfg.audience_path().is_empty() {
            let actual = json_path::as_string(&claims, cfg.audience_path()).unwrap_or_default();
            if actual.trim() != expected_audience {
                bail!(
                    "auth provider `{}`: ticket audience does not match",
                    self.provider_id
                );
            }
        }

        // Replay protection: the ticket id when the IdP provides one, else a
        // digest of the ticket itself (opaque tickets have no id).
        let ticket_id = json_path::as_string(&claims, cfg.ticket_id_path())
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| {
                let digest = Sha256::digest(raw_ticket.as_bytes());
                format!("sha256:{}", hex_encode(&digest))
            });
        let retain = path_i64(&claims, cfg.expires_at_path())
            .map(|exp| Duration::from_secs((exp - now).max(60) as u64 + 60))
            .unwrap_or(MIN_REPLAY_RETENTION);
        if !consume_once(&self.provider_id, &ticket_id, retain) {
            bail!("auth provider `{}`: ticket was already used", self.provider_id);
        }

        let issuer = if cfg.issuer_path.trim().is_empty() {
            self.config.issuer_opt()
        } else {
            json_path::as_string(&claims, cfg.issuer_path.trim()).or_else(|| self.config.issuer_opt())
        };

        Ok(Identity {
            provider_id: self.provider_id.clone(),
            issuer,
            subject,
            claims,
        })
    }
}

#[async_trait::async_trait]
impl AuthProviderDriver for TokenLookupDriver {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn kind(&self) -> AuthProviderKind {
        AuthProviderKind::TokenLookup
    }

    async fn authorize_url(&self, _request: &AuthorizeRequest<'_>) -> Result<Option<String>> {
        // The portal builds the redirect; we only receive the ticket.
        Ok(None)
    }

    async fn authenticate(&self, _request: AuthCodeRequest<'_>) -> Result<Identity> {
        bail!(
            "auth provider `{}`: this provider is entered with a ticket, not an authorization code",
            self.provider_id
        )
    }

    async fn authenticate_ticket(&self, ticket: &str) -> Result<Identity> {
        self.verify(ticket).await
    }
}

fn path_i64(value: &Value, path: &str) -> Option<i64> {
    if path.is_empty() {
        return None;
    }
    match json_path::get(value, path)? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn hmac_sha256(secret: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        let _ = a.ct_eq(a);
        return false;
    }
    a.ct_eq(b).into()
}

fn b64url_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_b64url(value: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim().trim_end_matches('='))
        .ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
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
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const AUDIENCE: &str = "https://agent.example.com";

    fn signed_config() -> AuthProviderConfig {
        toml::from_str(
            r#"
kind = "token_lookup"
display_name = "企业 SSO"
issuer = "https://portal.example.com"

[token_lookup]
ticket_param = "sso"
subject_path = "/sub"
nickname_path = "/name"
audience_path = "/aud"
expires_at_path = "/exp"
issued_at_path = "/iat"
ticket_id_path = "/jti"

[token_lookup.signing]
secret = "test-secret"
audience = "https://agent.example.com"
"#,
        )
        .expect("signed ticket config parses")
    }

    fn driver(config: AuthProviderConfig) -> TokenLookupDriver {
        TokenLookupDriver::new("corp_sso", config).expect("driver builds")
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    fn mint(secret: &str, audience: &str, subject: &str, ttl: i64) -> String {
        mint_signed_ticket(secret, audience, subject, Some("张三"), ttl, now()).expect("mint")
    }

    /// Sign arbitrary claims (lets tests build expired / malformed tickets).
    fn sign_claims(secret: &str, claims: Value) -> String {
        let header = b64url_encode(TICKET_HEADER_JSON.as_bytes());
        let payload = b64url_encode(claims.to_string().as_bytes());
        let signing_input = format!("{header}.{payload}");
        let signature = hmac_sha256(secret.as_bytes(), signing_input.as_bytes());
        format!("{signing_input}.{}", b64url_encode(&signature))
    }

    #[tokio::test]
    async fn signed_ticket_roundtrip() {
        let driver = driver(signed_config());
        assert_eq!(driver.ticket_param(), "sso");
        let ticket = mint("test-secret", AUDIENCE, "user-42", 120);
        let identity = driver.verify(&ticket).await.expect("verify");
        assert_eq!(identity.provider_id, "corp_sso");
        assert_eq!(identity.subject, "user-42");
        assert_eq!(identity.issuer.as_deref(), Some("https://portal.example.com"));
        assert_eq!(identity.claims["name"], json!("张三"));
    }

    #[tokio::test]
    async fn replayed_ticket_is_rejected() {
        let driver = driver(signed_config());
        let ticket = mint("test-secret", AUDIENCE, "user-replay", 120);
        driver.verify(&ticket).await.expect("first use");
        let err = driver
            .verify(&ticket)
            .await
            .expect_err("second use is a replay");
        assert!(err.to_string().contains("already used"), "{err}");
    }

    #[tokio::test]
    async fn rejects_wrong_secret_and_alg_none() {
        let driver = driver(signed_config());
        let err = driver
            .verify(&mint("other-secret", AUDIENCE, "u", 120))
            .await
            .expect_err("wrong secret is rejected");
        assert!(err.to_string().contains("signature"), "{err}");

        let header = b64url_encode(br#"{"alg":"none"}"#);
        let claims = b64url_encode(
            json!({"sub": "u", "aud": AUDIENCE, "exp": now() + 60}).to_string().as_bytes(),
        );
        let err = driver
            .verify(&format!("{header}.{claims}."))
            .await
            .expect_err("alg none is rejected");
        assert!(err.to_string().contains("not accepted"), "{err}");
    }

    #[tokio::test]
    async fn rejects_expired_ticket_and_wrong_audience() {
        let driver = driver(signed_config());
        let expired = sign_claims(
            "test-secret",
            json!({
                "sub": "u-exp",
                "aud": AUDIENCE,
                "iat": now() - 7200,
                "exp": now() - 3600,
            }),
        );
        let err = driver
            .verify(&expired)
            .await
            .expect_err("expired ticket is rejected");
        assert!(err.to_string().contains("expired"), "{err}");

        let err = driver
            .verify(&mint("test-secret", "https://other.example.com", "u-aud", 120))
            .await
            .expect_err("wrong audience is rejected");
        assert!(err.to_string().contains("audience"), "{err}");
    }

    #[tokio::test]
    async fn opaque_ticket_is_looked_up_and_mapped() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sso/verify"))
            .and(query_param("ticket", "opaque-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "userId": "lookup-user", "name": "李四" }
            })))
            .mount(&server)
            .await;

        let config: AuthProviderConfig = toml::from_str(&format!(
            r#"
kind = "token_lookup"

[token_lookup]
ticket_param = "ticket"
subject_path = "/data/userId"
nickname_path = "/data/name"

[token_lookup.lookup]
url = "{base}/sso/verify"
method = "get"
param = "ticket"
"#,
            base = server.uri()
        ))
        .expect("lookup config parses");

        let identity = driver(config)
            .verify("opaque-1")
            .await
            .expect("lookup login");
        assert_eq!(identity.subject, "lookup-user");
        assert_eq!(identity.claims["data"]["name"], json!("李四"));
    }

    #[tokio::test]
    async fn lookup_failure_is_reported() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sso/verify"))
            .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
            .mount(&server)
            .await;

        let config: AuthProviderConfig = toml::from_str(&format!(
            r#"
kind = "token_lookup"

[token_lookup.lookup]
url = "{base}/sso/verify"
"#,
            base = server.uri()
        ))
        .expect("lookup config parses");

        let err = driver(config)
            .verify("opaque-2")
            .await
            .expect_err("lookup failure surfaces");
        assert!(err.to_string().contains("HTTP 401"), "{err}");
    }

    #[test]
    fn config_requires_lookup_or_secret() {
        let config: AuthProviderConfig = toml::from_str(
            r#"
kind = "token_lookup"
"#,
        )
        .expect("parses");
        let err = config.validate("corp_sso").expect_err("no mode configured");
        assert!(err.to_string().contains("lookup.url"), "{err}");

        // Both at once is ambiguous and rejected.
        let config: AuthProviderConfig = toml::from_str(
            r#"
kind = "token_lookup"

[token_lookup.signing]
secret = "s"

[token_lookup.lookup]
url = "https://idp.example.com/verify"
"#,
        )
        .expect("parses");
        let err = config.validate("corp_sso").expect_err("both modes configured");
        assert!(err.to_string().contains("not both"), "{err}");
    }
}
