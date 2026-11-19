//! What a provider driver hands back, and the driver contract.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config::AuthProviderKind;

/// Verified third-party identity.
///
/// Deliberately session-free: drivers never touch cookies, web sessions, or
/// the access-control allowlist. The server layer decides what to do with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    /// Registry id the identity came from (`dingtalk`, `feishu`, …).
    pub provider_id: String,
    /// Provider issuer, from the response claim when configured, else config.
    pub issuer: Option<String>,
    /// Stable provider-side user id (`sub`, DingTalk `unionId`, …).
    pub subject: String,
    /// Raw userinfo JSON as returned by the provider.
    pub claims: Value,
}

/// Inputs for building the browser authorize URL.
#[derive(Debug, Clone)]
pub struct AuthorizeRequest<'a> {
    /// Absolute callback URL the provider will redirect back to.
    pub redirect_uri: &'a str,
    /// Opaque per-attempt token; the driver echoes it back as `state`.
    pub state: &'a str,
    /// PKCE S256 challenge, when the provider uses PKCE.
    pub code_challenge: Option<&'a str>,
    /// OIDC `nonce` binding the ID token to this attempt.
    pub nonce: Option<&'a str>,
}

/// Inputs for an authorization-code login.
#[derive(Debug, Clone)]
pub struct AuthCodeRequest<'a> {
    pub code: &'a str,
    /// Must match the `redirect_uri` used on the authorize request.
    pub redirect_uri: &'a str,
    /// PKCE verifier when the provider uses PKCE.
    pub code_verifier: Option<&'a str>,
    /// OIDC `nonce` sent on the authorize request; must match the ID token.
    pub nonce: Option<&'a str>,
}

/// Inputs for a direct credential login (username + password form).
#[derive(Debug, Clone)]
pub struct PasswordCredentials<'a> {
    pub username: &'a str,
    pub password: &'a str,
}

/// Turns a provider response into an [`Identity`], and builds the authorize URL
/// that starts the flow. Drivers are stateless: `state`, PKCE verifiers, nonces,
/// and ticket replay bookkeeping live in the provider layer, not here.
#[async_trait::async_trait]
pub trait AuthProviderDriver: Send + Sync {
    fn provider_id(&self) -> &str;

    fn kind(&self) -> AuthProviderKind;

    /// Browser redirect target that begins the code flow, or `None` when the
    /// provider is entered by other means (`local_password`, `token_lookup`).
    async fn authorize_url(&self, request: &AuthorizeRequest<'_>) -> Result<Option<String>>;

    /// Full redirect login: code → token → identity. Stateless, no persistence.
    async fn authenticate(&self, request: AuthCodeRequest<'_>) -> Result<Identity>;

    /// Direct credential login. Unsupported by default.
    async fn authenticate_password(&self, credentials: &PasswordCredentials<'_>) -> Result<Identity> {
        let _ = credentials;
        bail!(
            "auth provider `{}`: kind `{}` does not accept a password",
            self.provider_id(),
            self.kind().as_str()
        )
    }

    /// Inbound ticket login. Unsupported by default.
    async fn authenticate_ticket(&self, ticket: &str) -> Result<Identity> {
        let _ = ticket;
        bail!(
            "auth provider `{}`: kind `{}` does not accept a ticket",
            self.provider_id(),
            self.kind().as_str()
        )
    }
}
