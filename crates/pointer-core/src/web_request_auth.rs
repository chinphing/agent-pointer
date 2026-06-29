//! Per-request platform auth override for multi-user pointer-server (browser cookie sessions).
//!
//! Desktop / Tauri uses a single process-wide [`crate::platform_auth::PlatformAuthManager`].
//! pointer-server scopes OAuth state to the caller's browser via task-local overrides.

use crate::platform_auth::{PlatformAuthManager, PlatformLoginCredentials};
use std::future::Future;
use std::sync::Arc;
use tokio::task_local;

task_local! {
    static SCOPED_AUTH: Arc<PlatformAuthManager>;
    static SCOPED_CREDS: PlatformLoginCredentials;
}

/// Resolve the platform auth manager for the current async task.
pub fn scoped_auth(default: &Arc<PlatformAuthManager>) -> Arc<PlatformAuthManager> {
    SCOPED_AUTH.try_with(Arc::clone).ok().unwrap_or_else(|| default.clone())
}

/// Login credentials injected for the current request (server web session).
pub fn scoped_login_creds() -> Option<PlatformLoginCredentials> {
    SCOPED_CREDS.try_with(Clone::clone).ok()
}

/// Browser OAuth session captured from the current HTTP request (pointer-server).
#[derive(Clone)]
pub struct WebSessionAuth {
    pub auth: Arc<PlatformAuthManager>,
    pub creds: PlatformLoginCredentials,
}

impl std::fmt::Debug for WebSessionAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSessionAuth").finish_non_exhaustive()
    }
}

/// Capture per-browser auth when handling an HTTP request inside `web_session_middleware`.
pub fn capture_web_session_auth(default: &Arc<PlatformAuthManager>) -> Option<WebSessionAuth> {
    let creds = scoped_login_creds()?;
    Some(WebSessionAuth {
        auth: scoped_auth(default),
        creds,
    })
}

/// Run `f` with request-scoped auth + LLM credentials (pointer-server web UI).
pub async fn run_scoped<F, Fut, T>(
    auth: Arc<PlatformAuthManager>,
    creds: PlatformLoginCredentials,
    f: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    SCOPED_AUTH
        .scope(auth, async { SCOPED_CREDS.scope(creds, f()).await })
        .await
}

/// Propagate a captured web session into a spawned task (task-local does not inherit across `spawn`).
pub async fn run_with_optional_web_session<F, Fut, T>(
    ctx: Option<WebSessionAuth>,
    f: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    match ctx {
        Some(WebSessionAuth { auth, creds }) => run_scoped(auth, creds, f).await,
        None => f().await,
    }
}
