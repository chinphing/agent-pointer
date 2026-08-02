//! `im_send` tool — lets an agent proactively push a message to an IM channel
//! during a run. Registered by [`crate::bridge::install_channel_outbound_bridge`].
//!
//! This is the **dynamic** delivery path: the agent decides at runtime where to
//! send. The **static** path (cron job's `deliver` field → `ImDeliverHook`) is
//! handled separately in [`crate::im_deliver_hook`].
//!
//! Args:
//! - `to` (required): deliver spec, same format as cron `deliver` — e.g.
//!   `"feishu"`, `"feishu:ou_xxx"`, `"feishu:group:oc_xxx"`, comma-separated,
//!   `"all"`. See [`crate::im_delivery::resolve_delivery_targets`].
//! - `text` (required): the message body. Markdown is supported per channel.
//!
//! The tool is sync (`ToolHandler` signature); the outbound send is async, so
//! we bridge with `tokio::task::block_in_place` + `Handle::block_on`. Both
//! hosts (server `#[tokio::main]`, Tauri multi-threaded runtime) run a
//! multi-threaded scheduler, so `block_in_place` is safe.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use pointer_core::tools::{ToolEntry, ToolHandler, ToolRegistry};

use crate::chart_outbound::materialize_chartjs_fences_for_im;
use crate::gateway::ChannelGateway;
use crate::im_delivery::resolve_delivery_targets;
use crate::outbound_reply::split_reply_media;

const IM_SEND_MD: &str = include_str!("prompts/im_send.md");
const IM_SEND_DOC_SOURCE: &str = "tools/im_send/prompts/im_send.md";

/// Register the `im_send` tool. Called by [`crate::bridge::install_channel_outbound_bridge`].
pub fn register(reg: &ToolRegistry, gateway: Arc<ChannelGateway>) {
    let doc = IM_SEND_MD.trim();
    let gw = gateway.clone();
    let handler: ToolHandler = Arc::new(move |args: Value| -> Result<String> {
        dispatch(gw.clone(), &args)
    });

    reg.register(
        ToolEntry::new("im_send", IM_SEND_DOC_SOURCE, "low", false, doc, handler)
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Delivery target. Formats: \"feishu\" (home channel), \"feishu:ou_xxx\" (DM by open_id), \"feishu:group:chat_id\", \"dingtalk:userId\", \"wecom:userid\", \"weixin:wxid\", comma-separated for multiple, \"all\" for every configured home channel."
                    },
                    "text": {
                        "type": "string",
                        "description": "Message body (markdown supported per channel)."
                    }
                },
                "required": ["to", "text"]
            }))
            .with_subagent_inheritance(true),
    );
}

fn dispatch(gateway: Arc<ChannelGateway>, args: &Value) -> Result<String> {
    let to = args
        .get("to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("im_send: `to` is required"))?;
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("im_send: `text` is required"))?;

    let cfg = gateway.config().clone();
    let targets = resolve_delivery_targets(to, &cfg);
    if targets.is_empty() {
        return Err(anyhow!(
            "im_send: no IM targets resolved for {to:?} (check channel config / home recipient)"
        ));
    }

    // Chart fences → PNG MEDIA lines, then split like other IM outbound paths.
    let body = materialize_chartjs_fences_for_im(text);
    let (visible, media_refs) = split_reply_media(&body);
    if visible.trim().is_empty() && media_refs.is_empty() {
        return Err(anyhow!("im_send: empty message after chart/media processing"));
    }

    // Bridge sync handler → async outbound send. `block_in_place` parks the
    // current worker thread and runs the future on the runtime; safe on the
    // multi-threaded schedulers used by both hosts.
    let targets_clone: Vec<_> = targets
        .iter()
        .map(|t| {
            (
                t.channel.clone(),
                t.account_id.clone(),
                t.clone(),
                visible.clone(),
                media_refs.clone(),
            )
        })
        .collect();
    let outcome = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move {
            let total = targets_clone.len();
            let mut ok = 0usize;
            let mut errors: Vec<String> = Vec::new();
            for (channel, account, ctx, body, media) in targets_clone {
                match gateway
                    .send_outbound_explicit(&ctx, Some(&body), &media)
                    .await
                {
                    Ok(()) => {
                        ok += 1;
                        log::info!(
                            "im_send: delivered channel={} account={} recipient={}",
                            channel,
                            account,
                            ctx.recipient_id
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "im_send: target failed channel={} account={} recipient={}: {e:#}",
                            channel,
                            account,
                            ctx.recipient_id
                        );
                        errors.push(format!("{channel}: {e:#}"));
                    }
                }
            }
            (total, ok, errors)
        })
    });

    let (total, ok, errors) = outcome;
    Ok(json!({
        "ok": errors.is_empty(),
        "delivered": ok,
        "total": total,
        "errors": errors,
    })
    .to_string())
}
