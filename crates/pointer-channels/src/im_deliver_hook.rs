//! `OnRunFinishedHook` that delivers the agent's final reply to IM channels
//! based on a `deliver` spec stashed in `trigger_meta.extra.deliver` by the
//! trigger source (cron scheduler / HTTP Runs API / webhook ingress).
//!
//! The hook is best-effort: it never fails the run. Per-target errors are
//! logged and do not abort the remaining targets. This mirrors hermes'
//! `DeliveryRouter.deliver` semantics.
//!
//! Phase 1 wiring: the host (server / Tauri) builds the gateway before the
//! dispatcher, constructs `ImDeliverHook::new(gateway)`, and passes it to
//! `AppState::build_dispatcher_with_extra_hooks`.

use std::sync::Arc;

use anyhow::Result;

use pointer_core::dispatcher::hooks::{HookIdentity, OnRunFinishedHook, RunFinishedContext};
use pointer_core::dispatcher::TriggerMeta;

use crate::chart_outbound::materialize_chartjs_fences_for_im;
use crate::gateway::ChannelGateway;
use crate::im_delivery::{
    is_silence_narration, resolve_delivery_targets, truncate_for_platform, MAX_PLATFORM_OUTPUT,
};
use crate::im_mirror::mirror_delivery_to_im_session;
use crate::outbound_reply::split_reply_media;

/// Hook id / ordering. Sorts after the builtin lifecycle log hook
/// (`_10_lifecycle_log`) so the run-finished log line precedes delivery logs.
const OVERRIDE_KEY: &str = "channels.im_deliver";
const SORT_KEY: &str = "_20_im_deliver";

pub struct ImDeliverHook {
    gateway: Arc<ChannelGateway>,
}

impl ImDeliverHook {
    pub fn new(gateway: Arc<ChannelGateway>) -> Self {
        Self { gateway }
    }
}

impl HookIdentity for ImDeliverHook {
    fn override_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(OVERRIDE_KEY)
    }
    fn sort_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(SORT_KEY)
    }
}

#[async_trait::async_trait]
impl OnRunFinishedHook for ImDeliverHook {
    async fn execute(&self, ctx: &RunFinishedContext) -> Result<()> {
        // Best-effort: any internal error is logged and swallowed so the run
        // status is unaffected.
        if let Err(e) = self.deliver(ctx).await {
            log::warn!(
                "im_deliver_hook: run_id={} delivery aborted: {e:#}",
                ctx.run_id
            );
        }
        Ok(())
    }
}

impl ImDeliverHook {
    async fn deliver(&self, ctx: &RunFinishedContext) -> Result<()> {
        let run = ctx
            .state
            .session_index
            .runs_get(&ctx.run_id)
            .map_err(|e| anyhow::anyhow!("load run record failed: {e:#}"))?
            .ok_or_else(|| anyhow::anyhow!("run record not found for {}", ctx.run_id))?;

        let meta: TriggerMeta = serde_json::from_str(&run.trigger_meta_json)
            .map_err(|e| anyhow::anyhow!("parse trigger_meta failed: {e:#}"))?;

        let deliver = meta
            .extra
            .as_ref()
            .and_then(|v| v.get("deliver"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if deliver.is_empty() {
            return Ok(());
        }

        let content = match last_assistant_reply(&ctx.conversation_id, &ctx.state.session_index) {
            Some(c) => c,
            None => {
                log::info!(
                    "im_deliver_hook: run_id={} no assistant reply to deliver; skipping",
                    ctx.run_id
                );
                return Ok(());
            }
        };

        if is_silence_narration(&content) {
            log::info!(
                "im_deliver_hook: run_id={} silence narration; skipping delivery",
                ctx.run_id
            );
            clear_cron_delivery_error(&ctx.state, meta.job_id.as_deref());
            return Ok(());
        }

        let content = materialize_chartjs_fences_for_im(&content);
        let (visible, media_refs) = split_reply_media(&content);
        if visible.trim().is_empty() && media_refs.is_empty() {
            log::info!(
                "im_deliver_hook: run_id={} empty reply after media split; skipping",
                ctx.run_id
            );
            return Ok(());
        }

        let visible = if visible.chars().count() > MAX_PLATFORM_OUTPUT {
            truncate_for_platform(&visible)
        } else {
            visible
        };

        let cfg = self.gateway.config().clone();
        let targets = resolve_delivery_targets(deliver, &cfg);
        if targets.is_empty() {
            let msg = format!("deliver={deliver:?} resolved no targets");
            log::warn!("im_deliver_hook: run_id={} {msg}", ctx.run_id);
            set_cron_delivery_error(&ctx.state, meta.job_id.as_deref(), Some(&msg));
            return Ok(());
        }

        let total = targets.len();
        let mut ok = 0usize;
        let mut failed = Vec::new();
        for ctx_out in targets {
            let channel = ctx_out.channel.clone();
            let account = ctx_out.account_id.clone();
            let recipient = ctx_out.recipient_id.clone();
            match self
                .gateway
                .send_outbound_explicit(&ctx_out, Some(&visible), &media_refs)
                .await
            {
                Ok(()) => {
                    ok += 1;
                    log::info!(
                        "im_deliver_hook: delivered run_id={} channel={} account={} recipient={}",
                        ctx.run_id,
                        channel,
                        account,
                        recipient
                    );
                    mirror_delivery_to_im_session(
                        &ctx.state.session_index,
                        &ctx_out,
                        &visible,
                        &ctx.run_id,
                        meta.job_id.as_deref(),
                    );
                }
                Err(e) => {
                    log::warn!(
                        "im_deliver_hook: target failed run_id={} channel={} account={} recipient={}: {e:#}",
                        ctx.run_id,
                        channel,
                        account,
                        recipient
                    );
                    failed.push(format!("{channel}/{recipient}: {e:#}"));
                }
            }
        }
        log::info!(
            "im_deliver_hook: run_id={} delivery done ok={ok}/{total} failed={}",
            ctx.run_id,
            failed.len()
        );
        if failed.is_empty() {
            clear_cron_delivery_error(&ctx.state, meta.job_id.as_deref());
        } else {
            let summary = failed.join("; ");
            set_cron_delivery_error(&ctx.state, meta.job_id.as_deref(), Some(&summary));
        }
        Ok(())
    }
}

fn clear_cron_delivery_error(state: &pointer_core::chat_service::AppState, job_id: Option<&str>) {
    set_cron_delivery_error(state, job_id, None);
}

fn set_cron_delivery_error(
    state: &pointer_core::chat_service::AppState,
    job_id: Option<&str>,
    err: Option<&str>,
) {
    let Some(job_id) = job_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if let Err(e) = state
        .session_index
        .cron_jobs_set_last_delivery_error(job_id, err)
    {
        log::warn!("im_deliver_hook: set_last_delivery_error failed job_id={job_id}: {e:#}");
    }
}

/// Pull the last assistant message's outbound-facing content from the run's
/// transcript. Prefers `raw_content` (the model's verbatim reply, which may
/// include `MEDIA:` markers) and falls back to `content`.
fn last_assistant_reply(
    conversation_id: &str,
    store: &pointer_core::conversation_store::ConversationStore,
) -> Option<String> {
    match store.load_last_assistant_outbound_text(conversation_id) {
        Ok(text) => text,
        Err(error) => {
            log::warn!(
                "im_deliver_hook: last assistant lookup failed conversation_id={conversation_id}: {error:#}"
            );
            None
        }
    }
}
