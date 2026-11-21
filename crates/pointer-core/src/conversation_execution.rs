//! Per-conversation execution context and shared Environment leases.
//!
//! A user-level Environment may be shared across conversations. Isolation for
//! cancel, PTY, console, and tool cwd is applied here — not by giving each
//! conversation its own container.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::workspace_resolve::ResolvedWorkspace;

/// Conversation-scoped execution state (does not own the Environment).
#[derive(Clone, Debug)]
pub struct ConversationExecutionContext {
    pub conversation_id: String,
    pub owner_user_key: String,
    pub workspace: ResolvedWorkspace,
    /// Optional managed worktree guest/host path.
    pub worktree_root: Option<PathBuf>,
    /// Soft deadline hint for UI; hard cancel still uses JobSupervisor tokens.
    pub started_at: Instant,
}

impl ConversationExecutionContext {
    pub fn active_cwd(&self) -> &std::path::Path {
        self.workspace.active_root()
    }
}

fn contexts() -> &'static Mutex<HashMap<String, ConversationExecutionContext>> {
    static MAP: OnceLock<Mutex<HashMap<String, ConversationExecutionContext>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember the resolved workspace for a conversation turn / panel request.
pub fn remember(ctx: ConversationExecutionContext) {
    let id = ctx.conversation_id.clone();
    if let Ok(mut map) = contexts().lock() {
        map.insert(id, ctx);
    }
}

pub fn get(conversation_id: &str) -> Option<ConversationExecutionContext> {
    contexts()
        .lock()
        .ok()
        .and_then(|map| map.get(conversation_id.trim()).cloned())
}

pub fn forget(conversation_id: &str) {
    if let Ok(mut map) = contexts().lock() {
        map.remove(conversation_id.trim());
    }
}

/// Desktop / Computer control lease for one user Environment.
///
/// Only one conversation may hold the lease at a time. `pin` on the Environment
/// only prevents idle eviction — it is not a control lock.
#[derive(Clone, Debug)]
pub struct DesktopLease {
    pub owner_conversation_id: String,
    pub owner_user_key: String,
    pub acquired_at: Instant,
}

fn desktop_leases() -> &'static Mutex<HashMap<String, DesktopLease>> {
    static MAP: OnceLock<Mutex<HashMap<String, DesktopLease>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Try to acquire the desktop control lease for `user_key`.
pub fn try_acquire_desktop_lease(
    user_key: &str,
    conversation_id: &str,
) -> Result<DesktopLease> {
    let key = user_key.trim();
    let cid = conversation_id.trim();
    if key.is_empty() || cid.is_empty() {
        bail!("desktop lease requires user_key and conversation_id");
    }
    let mut map = desktop_leases()
        .lock()
        .map_err(|_| anyhow::anyhow!("desktop lease map poisoned"))?;
    if let Some(existing) = map.get(key) {
        if existing.owner_conversation_id == cid {
            return Ok(existing.clone());
        }
        bail!(
            "桌面已被会话 {} 占用，请稍后再试",
            existing.owner_conversation_id
        );
    }
    let lease = DesktopLease {
        owner_conversation_id: cid.to_string(),
        owner_user_key: key.to_string(),
        acquired_at: Instant::now(),
    };
    map.insert(key.to_string(), lease.clone());
    log::info!("desktop_lease: acquired user_key={key} conversation={cid}");
    Ok(lease)
}

pub fn release_desktop_lease(user_key: &str, conversation_id: &str) -> bool {
    let key = user_key.trim();
    let cid = conversation_id.trim();
    let Ok(mut map) = desktop_leases().lock() else {
        return false;
    };
    let Some(existing) = map.get(key) else {
        return false;
    };
    if existing.owner_conversation_id != cid {
        log::warn!(
            "desktop_lease: release ignored user_key={key} requester={cid} owner={}",
            existing.owner_conversation_id
        );
        return false;
    }
    map.remove(key);
    log::info!("desktop_lease: released user_key={key} conversation={cid}");
    true
}

pub fn desktop_lease_holder(user_key: &str) -> Option<DesktopLease> {
    desktop_leases()
        .lock()
        .ok()
        .and_then(|map| map.get(user_key.trim()).cloned())
}

/// Release conversation-scoped resources that do not destroy a shared Environment.
pub fn release_conversation_resources(conversation_id: &str) {
    let cid = conversation_id.trim();
    if cid.is_empty() {
        return;
    }
    forget(cid);
    // Drop any desktop lease held by this conversation.
    if let Ok(mut map) = desktop_leases().lock() {
        let keys: Vec<String> = map
            .iter()
            .filter(|(_, lease)| lease.owner_conversation_id == cid)
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys {
            map.remove(&key);
            log::info!("desktop_lease: released on conversation delete user_key={key} conversation={cid}");
        }
    }
    run_cleanup_hooks(cid);
}

type CleanupHook = Box<dyn Fn(&str) + Send + Sync>;

fn cleanup_hooks() -> &'static Mutex<Vec<CleanupHook>> {
    static HOOKS: OnceLock<Mutex<Vec<CleanupHook>>> = OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a conversation-delete cleanup hook (console close, etc.).
pub fn register_cleanup_hook<F>(hook: F)
where
    F: Fn(&str) + Send + Sync + 'static,
{
    if let Ok(mut hooks) = cleanup_hooks().lock() {
        hooks.push(Box::new(hook));
    }
}

fn run_cleanup_hooks(conversation_id: &str) {
    let Ok(hooks) = cleanup_hooks().lock() else {
        return;
    };
    for hook in hooks.iter() {
        hook(conversation_id);
    }
}

/// Soft age used by diagnostics / UI.
#[allow(dead_code)]
pub fn context_age(conversation_id: &str) -> Option<Duration> {
    get(conversation_id).map(|ctx| ctx.started_at.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_resolve::ResolvedWorkspace;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn sample_ctx(id: &str) -> ConversationExecutionContext {
        ConversationExecutionContext {
            conversation_id: id.into(),
            owner_user_key: "u1".into(),
            workspace: ResolvedWorkspace {
                owner_user_key: "u1".into(),
                data_root: Some(PathBuf::from("/data")),
                workspace_root: PathBuf::from("/data/workspaces/demo"),
                active_cwd: PathBuf::from("/data/workspaces/demo"),
            },
            worktree_root: None,
            started_at: Instant::now(),
        }
    }

    #[test]
    fn remember_and_forget_context() {
        remember(sample_ctx("conv-exec-1"));
        assert!(get("conv-exec-1").is_some());
        forget("conv-exec-1");
        assert!(get("conv-exec-1").is_none());
    }

    #[test]
    fn desktop_lease_is_exclusive() {
        let _ = release_desktop_lease("lease-user", "a");
        try_acquire_desktop_lease("lease-user", "a").unwrap();
        assert!(try_acquire_desktop_lease("lease-user", "b").is_err());
        assert!(release_desktop_lease("lease-user", "a"));
        try_acquire_desktop_lease("lease-user", "b").unwrap();
        assert!(release_desktop_lease("lease-user", "b"));
    }

    #[test]
    fn cleanup_hooks_run_on_release() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        register_cleanup_hook(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });
        release_conversation_resources("conv-hook-1");
        assert!(counter.load(Ordering::SeqCst) >= 1);
    }
}
