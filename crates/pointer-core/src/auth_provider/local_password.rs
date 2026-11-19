//! `local_password` driver: the operator's own username/password account.
//!
//! The account itself lives in `[auth.local]` (username + password HMAC +
//! hmac_secret, or the matching `POINTER_SERVER_ADMIN_*` env vars) and is
//! verified by [`crate::local_auth`]. This driver only adapts that check to the
//! provider contract, so the login page, the access gate, and the session exit
//! are the same code path as any federated provider.

use anyhow::{bail, Result};

use super::config::{AuthProviderConfig, AuthProviderKind};
use super::identity::{
    AuthCodeRequest, AuthProviderDriver, AuthorizeRequest, Identity, PasswordCredentials,
};

/// Username/password provider. Not a redirect flow: the SPA posts credentials.
#[derive(Debug)]
pub struct LocalPasswordDriver {
    provider_id: String,
    config: AuthProviderConfig,
}

impl LocalPasswordDriver {
    pub fn new(provider_id: impl Into<String>, config: AuthProviderConfig) -> Result<Self> {
        let provider_id = provider_id.into();
        if config.kind != AuthProviderKind::LocalPassword {
            bail!(
                "auth provider `{provider_id}`: kind `{}` is not served by this driver",
                config.kind.as_str()
            );
        }
        config.validate(&provider_id)?;
        Ok(Self {
            provider_id,
            config,
        })
    }

    pub fn config(&self) -> &AuthProviderConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl AuthProviderDriver for LocalPasswordDriver {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn kind(&self) -> AuthProviderKind {
        AuthProviderKind::LocalPassword
    }

    async fn authorize_url(&self, _request: &AuthorizeRequest<'_>) -> Result<Option<String>> {
        // Nothing to redirect to: the login page renders a form instead.
        Ok(None)
    }

    async fn authenticate(&self, _request: AuthCodeRequest<'_>) -> Result<Identity> {
        bail!(
            "auth provider `{}`: local password login posts credentials, not an authorization code",
            self.provider_id
        )
    }

    async fn authenticate_password(&self, credentials: &PasswordCredentials<'_>) -> Result<Identity> {
        if !crate::local_auth::verify_local_password(credentials.username, credentials.password) {
            // Same error for "not configured" and "wrong password": the caller
            // only ever sees one code, and the log line above says which.
            bail!("invalid_credentials");
        }
        let nickname = crate::local_auth::configured_admin_username();
        let nickname = if nickname.is_empty() {
            "Admin".to_string()
        } else {
            nickname
        };
        Ok(Identity {
            provider_id: self.provider_id.clone(),
            issuer: None,
            subject: crate::local_auth::local_user_id().to_string(),
            claims: serde_json::json!({
                "sub": crate::local_auth::local_user_id(),
                "name": nickname,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver() -> LocalPasswordDriver {
        LocalPasswordDriver::new(
            "local_password",
            AuthProviderConfig::builtin(AuthProviderKind::LocalPassword, "本地密码"),
        )
        .expect("driver builds")
    }

    #[tokio::test]
    async fn has_no_redirect_entry() {
        let url = driver()
            .authorize_url(&AuthorizeRequest {
                redirect_uri: "https://pointer.example/cb",
                state: "s",
                code_challenge: None,
                nonce: None,
            })
            .await
            .expect("authorize url");
        assert!(url.is_none());
    }

    #[tokio::test]
    async fn rejects_an_authorization_code() {
        let err = driver()
            .authenticate(AuthCodeRequest {
                code: "abc",
                redirect_uri: "https://pointer.example/cb",
                code_verifier: None,
                nonce: None,
            })
            .await
            .expect_err("local password is not a code flow");
        assert!(err.to_string().contains("credentials"), "{err}");
    }

    #[tokio::test]
    async fn rejects_unconfigured_credentials() {
        // The suite runs without [auth.local]; nothing can verify.
        let err = driver()
            .authenticate_password(&PasswordCredentials {
                username: "admin",
                password: "whatever",
            })
            .await
            .expect_err("unconfigured account cannot log in");
        assert!(err.to_string().contains("invalid_credentials"), "{err}");
    }

    #[tokio::test]
    async fn rejects_a_wrong_kind() {
        let err = LocalPasswordDriver::new(
            "corp",
            AuthProviderConfig::builtin(AuthProviderKind::Platform, "平台账号"),
        )
        .expect_err("kind mismatch is rejected");
        assert!(err.to_string().contains("not served by this driver"), "{err}");
    }
}
