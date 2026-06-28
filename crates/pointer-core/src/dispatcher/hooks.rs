//! Dispatcher hook registry: lifecycle hooks at the dispatch boundary.
//!
//! This is the dispatcher-level companion to [`crate::extensions::ExtensionRegistry`].
//! The split keeps concerns separated:
//!
//! - [`crate::extensions::ExtensionRegistry`] — LLM-loop prompt injection hooks,
//!   tightly coupled to chat-internal contexts (computer state, task board, …).
//! - [`HookRegistry`] (this module) — dispatcher boundary hooks that see a
//!   run-level view (`run_id`, `conversation_id`, `TriggerRequest`) and can
//!   gate / rewrite triggers without knowing LLM-loop internals.
//!
//! Hook model borrows from hermes `VALID_HOOKS` (typed lifecycle points with
//! `override_key` replace semantics + `sort_key` ordering) and openclaw
//! `api.on(...)` (interception at dispatch boundaries).
//!
//! Phase 2 wires the six lifecycle hooks below. `pre_tool_call` /
//! `post_tool_call` traits are declared here too, but firing them from the
//! tool pass is deferred to Phase 3 (it requires threading the registry into
//! the chat context, which is done together with the caller migration to keep
//! Phase 2 low-risk).

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::chat_service::AppState;
use crate::dispatcher::trigger::TriggerRequest;

/// Outcome for hooks that can gate or rewrite a trigger.
#[derive(Debug)]
pub enum HookOutcome {
    /// Proceed with the (possibly rewritten) request.
    Continue,
    /// Replace the request with a new one. Only meaningful for
    /// [`OnTriggerReceivedHook`]; ignored by other hooks.
    Rewrite(TriggerRequest),
    /// Abort the dispatch with a reason. The dispatcher returns the reason as
    /// an error (for trigger-received / pre-dispatch) or logs it (observation
    /// hooks ignore reject).
    Reject { reason: String },
}

impl HookOutcome {
    pub fn continue_() -> Self {
        HookOutcome::Continue
    }

    pub fn reject(reason: impl Into<String>) -> Self {
        HookOutcome::Reject {
            reason: reason.into(),
        }
    }
}

// ---- Contexts ----

/// Context for [`OnTriggerReceivedHook`]. Owns the request so a hook can
/// return a rewritten copy via [`HookOutcome::Rewrite`].
pub struct TriggerReceivedContext {
    pub req: TriggerRequest,
}

/// Context for [`PreDispatchHook`]. Fires after idempotency check and run-id
/// resolution, before the run is enqueued. A hook may reject (abort) but not
/// rewrite (the request is settled by this point).
pub struct PreDispatchContext<'a> {
    pub run_id: &'a str,
    pub conversation_id: &'a str,
    pub lane: &'a str,
    pub trigger_source: crate::dispatcher::TriggerSource,
}

/// Context for observation hooks. All share `state` so built-in hooks (e.g.
/// the memory review in future phases) can reach shared stores.
pub struct RunStartedContext {
    pub run_id: String,
    pub conversation_id: String,
    pub state: Arc<AppState>,
}

pub struct RunFinishedContext {
    pub run_id: String,
    pub conversation_id: String,
    pub state: Arc<AppState>,
}

pub struct RunFailedContext {
    pub run_id: String,
    pub conversation_id: String,
    pub error: String,
    pub state: Arc<AppState>,
}

pub struct RunCancelledContext {
    pub run_id: String,
    pub conversation_id: String,
    pub state: Arc<AppState>,
}

/// Context for [`PreToolCallHook`] (firing wired in Phase 3).
pub struct PreToolCallContext<'a> {
    pub run_id: &'a str,
    pub conversation_id: &'a str,
    pub message_id: &'a str,
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    pub args: &'a serde_json::Value,
    pub state: &'a Arc<AppState>,
}

/// Context for [`PostToolCallHook`] (firing wired in Phase 3).
pub struct PostToolCallContext<'a> {
    pub run_id: &'a str,
    pub conversation_id: &'a str,
    pub message_id: &'a str,
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    pub status: &'a str,
    pub result: Option<&'a str>,
    pub error: Option<&'a str>,
    pub state: &'a Arc<AppState>,
}

// ---- Traits ----

/// Stable identity + ordering shared by all hooks (mirrors
/// `ExtensionRegistry` semantics: re-registering the same `override_key`
/// replaces the prior hook; `sort_key` orders execution within a point).
pub trait HookIdentity: Send + Sync {
    fn override_key(&self) -> &'static str;
    fn sort_key(&self) -> &'static str;
}

#[async_trait]
pub trait OnTriggerReceivedHook: HookIdentity {
    async fn execute(&self, ctx: &mut TriggerReceivedContext) -> Result<HookOutcome>;
}

#[async_trait]
pub trait PreDispatchHook: HookIdentity {
    async fn execute(&self, ctx: &PreDispatchContext<'_>) -> Result<HookOutcome>;
}

#[async_trait]
pub trait OnRunStartedHook: HookIdentity {
    async fn execute(&self, ctx: &RunStartedContext) -> Result<()>;
}

#[async_trait]
pub trait OnRunFinishedHook: HookIdentity {
    async fn execute(&self, ctx: &RunFinishedContext) -> Result<()>;
}

#[async_trait]
pub trait OnRunFailedHook: HookIdentity {
    async fn execute(&self, ctx: &RunFailedContext) -> Result<()>;
}

#[async_trait]
pub trait OnRunCancelledHook: HookIdentity {
    async fn execute(&self, ctx: &RunCancelledContext) -> Result<()>;
}

#[async_trait]
pub trait PreToolCallHook: HookIdentity {
    async fn execute(&self, ctx: &PreToolCallContext<'_>) -> Result<HookOutcome>;
}

#[async_trait]
pub trait PostToolCallHook: HookIdentity {
    async fn execute(&self, ctx: &PostToolCallContext<'_>) -> Result<()>;
}

// ---- Registry ----

/// Registry of dispatcher hooks. Owned by [`crate::dispatcher::RunDispatcher`]
/// as `Arc<HookRegistry>`. Cheap to share.
#[derive(Default)]
pub struct HookRegistry {
    on_trigger_received: Vec<Arc<dyn OnTriggerReceivedHook>>,
    pre_dispatch: Vec<Arc<dyn PreDispatchHook>>,
    on_run_started: Vec<Arc<dyn OnRunStartedHook>>,
    on_run_finished: Vec<Arc<dyn OnRunFinishedHook>>,
    on_run_failed: Vec<Arc<dyn OnRunFailedHook>>,
    on_run_cancelled: Vec<Arc<dyn OnRunCancelledHook>>,
    pre_tool_call: Vec<Arc<dyn PreToolCallHook>>,
    post_tool_call: Vec<Arc<dyn PostToolCallHook>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_on_trigger_received(&mut self, hook: Arc<dyn OnTriggerReceivedHook>) {
        let key = hook.override_key();
        self.on_trigger_received.retain(|h| h.override_key() != key);
        self.on_trigger_received.push(hook);
    }

    pub fn register_pre_dispatch(&mut self, hook: Arc<dyn PreDispatchHook>) {
        let key = hook.override_key();
        self.pre_dispatch.retain(|h| h.override_key() != key);
        self.pre_dispatch.push(hook);
    }

    pub fn register_on_run_started(&mut self, hook: Arc<dyn OnRunStartedHook>) {
        let key = hook.override_key();
        self.on_run_started.retain(|h| h.override_key() != key);
        self.on_run_started.push(hook);
    }

    pub fn register_on_run_finished(&mut self, hook: Arc<dyn OnRunFinishedHook>) {
        let key = hook.override_key();
        self.on_run_finished.retain(|h| h.override_key() != key);
        self.on_run_finished.push(hook);
    }

    pub fn register_on_run_failed(&mut self, hook: Arc<dyn OnRunFailedHook>) {
        let key = hook.override_key();
        self.on_run_failed.retain(|h| h.override_key() != key);
        self.on_run_failed.push(hook);
    }

    pub fn register_on_run_cancelled(&mut self, hook: Arc<dyn OnRunCancelledHook>) {
        let key = hook.override_key();
        self.on_run_cancelled.retain(|h| h.override_key() != key);
        self.on_run_cancelled.push(hook);
    }

    pub fn register_pre_tool_call(&mut self, hook: Arc<dyn PreToolCallHook>) {
        let key = hook.override_key();
        self.pre_tool_call.retain(|h| h.override_key() != key);
        self.pre_tool_call.push(hook);
    }

    pub fn register_post_tool_call(&mut self, hook: Arc<dyn PostToolCallHook>) {
        let key = hook.override_key();
        self.post_tool_call.retain(|h| h.override_key() != key);
        self.post_tool_call.push(hook);
    }

    /// Run `on_trigger_received` hooks in sort order. The first `Reject`
    /// aborts; `Rewrite` replaces the request for subsequent hooks; `Continue`
    /// passes through. A hook returning `Err` aborts with that error.
    pub async fn run_on_trigger_received(
        &self,
        ctx: &mut TriggerReceivedContext,
    ) -> Result<HookOutcome> {
        let mut hooks = self.sorted_trigger_received();
        for h in hooks.drain(..) {
            match h.execute(ctx).await? {
                HookOutcome::Continue => {}
                HookOutcome::Rewrite(new_req) => {
                    ctx.req = new_req;
                }
                HookOutcome::Reject { reason } => {
                    log::info!(
                        "hook on_trigger_received: rejected by {} reason={}",
                        h.override_key(),
                        reason
                    );
                    return Ok(HookOutcome::Reject { reason });
                }
            }
        }
        Ok(HookOutcome::Continue)
    }

    /// Run `pre_dispatch` hooks. First `Reject` aborts.
    pub async fn run_pre_dispatch(&self, ctx: &PreDispatchContext<'_>) -> Result<HookOutcome> {
        let mut hooks = self.sorted_pre_dispatch();
        for h in hooks.drain(..) {
            match h.execute(ctx).await? {
                HookOutcome::Continue => {}
                HookOutcome::Rewrite(_) => {
                    log::warn!(
                        "hook pre_dispatch: {} returned Rewrite which is ignored at this point",
                        h.override_key()
                    );
                }
                HookOutcome::Reject { reason } => {
                    log::info!(
                        "hook pre_dispatch: rejected by {} reason={}",
                        h.override_key(),
                        reason
                    );
                    return Ok(HookOutcome::Reject { reason });
                }
            }
        }
        Ok(HookOutcome::Continue)
    }

    /// Observation hooks: errors are warn-logged, never propagated, so a
    /// faulty observer cannot break a run (project rule: errors that don't
    /// affect subsequent execution need warning logs, not throws).
    pub async fn run_on_run_started(&self, ctx: &RunStartedContext) {
        let mut hooks = self.sorted_run_started();
        for h in hooks.drain(..) {
            if let Err(e) = h.execute(ctx).await {
                log::warn!(
                    "hook on_run_started {} failed (ignored): {e:#}",
                    h.override_key()
                );
            }
        }
    }

    pub async fn run_on_run_finished(&self, ctx: &RunFinishedContext) {
        let mut hooks = self.sorted_run_finished();
        for h in hooks.drain(..) {
            if let Err(e) = h.execute(ctx).await {
                log::warn!(
                    "hook on_run_finished {} failed (ignored): {e:#}",
                    h.override_key()
                );
            }
        }
    }

    pub async fn run_on_run_failed(&self, ctx: &RunFailedContext) {
        let mut hooks = self.sorted_run_failed();
        for h in hooks.drain(..) {
            if let Err(e) = h.execute(ctx).await {
                log::warn!(
                    "hook on_run_failed {} failed (ignored): {e:#}",
                    h.override_key()
                );
            }
        }
    }

    pub async fn run_on_run_cancelled(&self, ctx: &RunCancelledContext) {
        let mut hooks = self.sorted_run_cancelled();
        for h in hooks.drain(..) {
            if let Err(e) = h.execute(ctx).await {
                log::warn!(
                    "hook on_run_cancelled {} failed (ignored): {e:#}",
                    h.override_key()
                );
            }
        }
    }

    // ---- pre/post_tool_call firing (Phase 3 wires the call sites) ----

    pub async fn run_pre_tool_call(
        &self,
        ctx: &PreToolCallContext<'_>,
    ) -> Result<HookOutcome> {
        let mut hooks = self.sorted_pre_tool_call();
        for h in hooks.drain(..) {
            match h.execute(ctx).await? {
                HookOutcome::Continue => {}
                HookOutcome::Rewrite(_) => {
                    log::warn!(
                        "hook pre_tool_call: {} returned Rewrite which is not yet supported",
                        h.override_key()
                    );
                }
                HookOutcome::Reject { reason } => {
                    return Ok(HookOutcome::Reject { reason });
                }
            }
        }
        Ok(HookOutcome::Continue)
    }

    pub async fn run_post_tool_call(&self, ctx: &PostToolCallContext<'_>) {
        let mut hooks = self.sorted_post_tool_call();
        for h in hooks.drain(..) {
            if let Err(e) = h.execute(ctx).await {
                log::warn!(
                    "hook post_tool_call {} failed (ignored): {e:#}",
                    h.override_key()
                );
            }
        }
    }

    // ---- sort helpers ----

    fn sorted_trigger_received(&self) -> Vec<Arc<dyn OnTriggerReceivedHook>> {
        let mut v: Vec<_> = self.on_trigger_received.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_pre_dispatch(&self) -> Vec<Arc<dyn PreDispatchHook>> {
        let mut v: Vec<_> = self.pre_dispatch.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_run_started(&self) -> Vec<Arc<dyn OnRunStartedHook>> {
        let mut v: Vec<_> = self.on_run_started.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_run_finished(&self) -> Vec<Arc<dyn OnRunFinishedHook>> {
        let mut v: Vec<_> = self.on_run_finished.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_run_failed(&self) -> Vec<Arc<dyn OnRunFailedHook>> {
        let mut v: Vec<_> = self.on_run_failed.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_run_cancelled(&self) -> Vec<Arc<dyn OnRunCancelledHook>> {
        let mut v: Vec<_> = self.on_run_cancelled.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_pre_tool_call(&self) -> Vec<Arc<dyn PreToolCallHook>> {
        let mut v: Vec<_> = self.pre_tool_call.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
    fn sorted_post_tool_call(&self) -> Vec<Arc<dyn PostToolCallHook>> {
        let mut v: Vec<_> = self.post_tool_call.iter().cloned().collect();
        v.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
        v
    }
}

// ---- Built-in hooks ----

/// Built-in observer that logs run lifecycle transitions at info level.
///
/// Registered by [`register_builtin_hooks`]. This is the Phase 2 demonstration
/// hook; it also satisfies the project observability rule (key execution
/// positions need info logs). Future built-ins (memory review, notifications)
/// register alongside it with distinct `override_key`s.
pub struct LifecycleLogHook;

impl HookIdentity for LifecycleLogHook {
    fn override_key(&self) -> &'static str {
        "builtin.lifecycle_log"
    }
    fn sort_key(&self) -> &'static str {
        "_10_lifecycle_log"
    }
}

#[async_trait]
impl OnRunStartedHook for LifecycleLogHook {
    async fn execute(&self, ctx: &RunStartedContext) -> Result<()> {
        log::info!(
            "hook lifecycle: run_started run_id={} conversation_id={}",
            ctx.run_id,
            ctx.conversation_id
        );
        Ok(())
    }
}

#[async_trait]
impl OnRunFinishedHook for LifecycleLogHook {
    async fn execute(&self, ctx: &RunFinishedContext) -> Result<()> {
        log::info!(
            "hook lifecycle: run_finished run_id={} conversation_id={}",
            ctx.run_id,
            ctx.conversation_id
        );
        Ok(())
    }
}

#[async_trait]
impl OnRunFailedHook for LifecycleLogHook {
    async fn execute(&self, ctx: &RunFailedContext) -> Result<()> {
        log::warn!(
            "hook lifecycle: run_failed run_id={} conversation_id={} error={}",
            ctx.run_id,
            ctx.conversation_id,
            ctx.error
        );
        Ok(())
    }
}

#[async_trait]
impl OnRunCancelledHook for LifecycleLogHook {
    async fn execute(&self, ctx: &RunCancelledContext) -> Result<()> {
        log::info!(
            "hook lifecycle: run_cancelled run_id={} conversation_id={}",
            ctx.run_id,
            ctx.conversation_id
        );
        Ok(())
    }
}

/// Register framework-default hooks (lifecycle logging). Hosts call this on a
/// fresh `HookRegistry` before wrapping it in `Arc` and passing to
/// [`crate::dispatcher::RunDispatcher::with_hooks`].
pub fn register_builtin_hooks(registry: &mut HookRegistry) {
    let log_hook: Arc<LifecycleLogHook> = Arc::new(LifecycleLogHook);
    registry.register_on_run_started(log_hook.clone());
    registry.register_on_run_finished(log_hook.clone());
    registry.register_on_run_failed(log_hook.clone());
    registry.register_on_run_cancelled(log_hook.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::trigger::{DeliverTarget, TriggerMeta, TriggerSource};
    use crate::models::ChatMessage;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StaticId {
        key: &'static str,
        sk: &'static str,
    }
    impl HookIdentity for StaticId {
        fn override_key(&self) -> &'static str {
            self.key
        }
        fn sort_key(&self) -> &'static str {
            self.sk
        }
    }

    fn sample_req() -> TriggerRequest {
        TriggerRequest {
            run_id: None,
            idempotency_key: None,
            conversation_id: Some("c1".into()),
            trigger_source: TriggerSource::Ipc,
            trigger_meta: TriggerMeta::empty(),
            lane: None,
            messages: Vec::<ChatMessage>::new(),
            enabled_skill_ids: vec![],
            agent_mode: None,
            lead_agent_id: None,
            tool_rounds_used_single_start: 0,
            tool_rounds_used_supervisor_start: 0,
            workspace_root: String::new(),
            workspace_inherit_disabled: None,
            deliver: DeliverTarget::None,
        }
    }

    #[tokio::test]
    async fn trigger_received_rewrite_chains_and_replaces() {
        let counter = Arc::new(AtomicUsize::new(0));
        struct RewriteHook {
            id: StaticId,
            new_lane: Option<String>,
            counter: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl OnTriggerReceivedHook for RewriteHook {
            async fn execute(&self, ctx: &mut TriggerReceivedContext) -> Result<HookOutcome> {
                self.counter.fetch_add(1, Ordering::SeqCst);
                if let Some(lane) = self.new_lane.clone() {
                    ctx.req.lane = Some(lane);
                }
                Ok(HookOutcome::Continue)
            }
        }
        impl HookIdentity for RewriteHook {
            fn override_key(&self) -> &'static str {
                self.id.override_key()
            }
            fn sort_key(&self) -> &'static str {
                self.id.sort_key()
            }
        }

        let mut reg = HookRegistry::new();
        reg.register_on_trigger_received(Arc::new(RewriteHook {
            id: StaticId { key: "a", sk: "_10" },
            new_lane: Some("lane-a".into()),
            counter: counter.clone(),
        }));
        reg.register_on_trigger_received(Arc::new(RewriteHook {
            id: StaticId { key: "b", sk: "_20" },
            new_lane: Some("lane-b".into()),
            counter: counter.clone(),
        }));

        let mut ctx = TriggerReceivedContext {
            req: sample_req(),
        };
        let outcome = reg.run_on_trigger_received(&mut ctx).await.unwrap();
        assert!(matches!(outcome, HookOutcome::Continue));
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(ctx.req.lane.as_deref(), Some("lane-b"));
    }

    #[tokio::test]
    async fn trigger_received_reject_aborts() {
        struct RejectHook(StaticId);
        #[async_trait]
        impl OnTriggerReceivedHook for RejectHook {
            async fn execute(&self, _ctx: &mut TriggerReceivedContext) -> Result<HookOutcome> {
                Ok(HookOutcome::reject("nope"))
            }
        }
        impl HookIdentity for RejectHook {
            fn override_key(&self) -> &'static str {
                self.0.override_key()
            }
            fn sort_key(&self) -> &'static str {
                self.0.sort_key()
            }
        }
        let mut reg = HookRegistry::new();
        reg.register_on_trigger_received(Arc::new(RejectHook(StaticId {
            key: "r",
            sk: "_10",
        })));
        let mut ctx = TriggerReceivedContext {
            req: sample_req(),
        };
        let outcome = reg.run_on_trigger_received(&mut ctx).await.unwrap();
        assert!(matches!(outcome, HookOutcome::Reject { .. }));
    }
}
