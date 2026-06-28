//! Webhook bearer token config (Phase 6): UI-settable, encrypted-at-rest,
//! first-write-only.
//!
//! Storage: the token is AES-256-GCM encrypted via [`crate::local_secret`]
//! (per-machine key, same scheme as `auth.dat`) and persisted as a blob in the
//! `app_secrets` kv table under [`Self::LABEL`].
//!
//! Lifecycle: the token can be set exactly once from the UI
//! ([`WebhookTokenStore::set_token`] refuses if already configured). The UI
//! shows a masked preview ([`WebhookTokenStore::preview`]) after it is set.
//! To rotate, an admin must explicitly clear it via
//! [`WebhookTokenStore::clear_token`] first.
//!
//! Resolution at request time ([`WebhookTokenStore::resolve`]): the db token
//! takes precedence; the `POINTER_WEBHOOK_BEARER_TOKEN` env var is a fallback
//! so existing deployments keep working without UI configuration.

use anyhow::Result;

use crate::conversation_store::ConversationStore;

const LABEL: &str = "webhook_bearer_token";

/// JSON view of the webhook token config (camelCase) shared by the server
/// HTTP API and the Tauri IPC commands. `url_template` is host-supplied
/// (empty on desktop where there is no HTTP ingress).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfigView {
    pub configured: bool,
    pub preview: Option<String>,
    pub url_template: String,
}

/// Persisted, encrypted webhook bearer token store backed by the conversation
/// store. Cheap to construct; re-reads the db on each call (token checks are
/// low-frequency).
pub struct WebhookTokenStore<'a> {
    store: &'a ConversationStore,
}

impl<'a> WebhookTokenStore<'a> {
    pub fn new(store: &'a ConversationStore) -> Self {
        Self { store }
    }

    /// Set the token. Refuses (returns `false`) if a token is already
    /// configured — first-write-only. Empty / whitespace tokens are rejected.
    pub fn set_token(&self, token: &str) -> Result<bool> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            anyhow::bail!("webhook token must not be empty");
        }
        if self.is_configured()? {
            log::warn!("webhook_config: set_token refused (already configured)");
            return Ok(false);
        }
        let blob = crate::local_secret::encrypt_local_secret(trimmed)?;
        let inserted = self.store.app_secret_try_insert(LABEL, &blob)?;
        if inserted {
            log::info!("webhook_config: bearer token set (first-write, encrypted)");
        } else {
            log::warn!("webhook_config: set_token lost race (already configured)");
        }
        Ok(inserted)
    }

    /// Whether a token is configured in the db.
    pub fn is_configured(&self) -> Result<bool> {
        self.store.app_secret_has(LABEL)
    }

    /// Read and decrypt the configured token, or `None` if unset / undecryptable.
    pub fn get_token(&self) -> Result<Option<String>> {
        let Some(blob) = self.store.app_secret_get(LABEL)? else {
            return Ok(None);
        };
        match crate::local_secret::decrypt_local_secret(&blob) {
            Ok(s) if !s.is_empty() => Ok(Some(s)),
            Ok(_) => Ok(None),
            Err(e) => {
                log::warn!("webhook_config: decrypt failed: {e}");
                Ok(None)
            }
        }
    }

    /// Masked preview for UI display: shows the last 4 chars prefixed with
/// asterisks, e.g. `****1234`. Returns `None` if not configured.
    pub fn preview(&self) -> Result<Option<String>> {
        let Some(token) = self.get_token()? else {
            return Ok(None);
        };
        let len = token.chars().count();
        if len <= 4 {
            return Ok(Some("****".into()));
        }
        let tail: String = token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        Ok(Some(format!("****{tail}")))
    }

    /// Resolve the effective token at request time: db token first, env
    /// `POINTER_WEBHOOK_BEARER_TOKEN` fallback. Returns `None` if neither is
    /// set (caller rejects with 401).
    pub fn resolve(&self) -> Result<Option<String>> {
        if let Some(t) = self.get_token()? {
            return Ok(Some(t));
        }
        let env_token = std::env::var("POINTER_WEBHOOK_BEARER_TOKEN").unwrap_or_default();
        if env_token.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(env_token))
        }
    }

    /// Remove the configured token (admin reset path). After this, the token
    /// can be set again from the UI.
    pub fn clear_token(&self) -> Result<bool> {
        let deleted = self.store.app_secret_delete(LABEL)?;
        if deleted {
            log::info!("webhook_config: bearer token cleared");
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::ConversationStore;

    fn store() -> ConversationStore {
        // In-memory isn't supported by the global store path; use a temp dir.
        let dir = std::env::temp_dir().join(format!(
            "pointer-webhook-config-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let s = ConversationStore::open(dir.join("conversations.db")).unwrap();
        // Clean the test token row left by other tests sharing the global db.
        let _ = s.app_secret_delete(LABEL);
        s
    }

    #[test]
    fn first_write_only_and_preview() {
        let s = store();
        let ts = WebhookTokenStore::new(&s);
        assert!(!ts.is_configured().unwrap());
        assert!(ts.set_token("secret-abcd1234").unwrap());
        assert!(ts.is_configured().unwrap());
        // Second set refused.
        assert!(!ts.set_token("other").unwrap());
        // Preview masks all but last 4.
        assert_eq!(ts.preview().unwrap().as_deref(), Some("****1234"));
        // Resolve returns the configured token.
        assert_eq!(ts.resolve().unwrap().as_deref(), Some("secret-abcd1234"));
        // Clear then re-set works.
        assert!(ts.clear_token().unwrap());
        assert!(ts.set_token("newtoken9999").unwrap());
    }

    #[test]
    fn empty_token_rejected() {
        let s = store();
        let ts = WebhookTokenStore::new(&s);
        assert!(ts.set_token("   ").is_err());
    }
}
