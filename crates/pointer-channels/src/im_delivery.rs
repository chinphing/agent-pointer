//! Run → IM delivery resolver.
//!
//! Parses a `deliver` string (persisted on cron jobs / HTTP runs / webhook
//! ingress as `trigger_meta.extra.deliver`) into one or more
//! [`OutboundContext`]s that [`ChannelGateway::send_outbound_explicit`] can
//! push to. The format is inspired by hermes' `DeliveryTarget`:
//!
//! - `"feishu"` — the channel's configured home channel (per account)
//! - `"feishu:ou_xxx"` — Feishu DM by open_id
//! - `"feishu:group:oc_xxx"` — Feishu group by chat_id
//! - `"dingtalk:userId"` — DingTalk DM by userId
//! - `"dingtalk:group:openConversationId"` — DingTalk group
//! - `"wecom:userid"` — WeCom DM by userid (Agent HTTP `message/send`)
//! - `"wecom:group:chat_id"` — WeCom group (WSS `send_markdown`)
//! - `"weixin:wxid"` — Weixin DM by wxid (needs cached `context_token`)
//! - comma-separated for multiple targets, e.g. `"feishu:ou_a,feishu:group:oc_b"`
//! - `"all"` — every configured channel's home channel
//!
//! Channel-specific differences (DingTalk group needs `openConversationId` in
//! `reply_context.chat_id`; WeCom group prefers `reply_context.chat_id` for
//! WSS `send_markdown`) are absorbed here so the delivery hook and the
//! `im_send` tool only call `send_outbound_explicit`.

use crate::config::ChannelsConfig;
use crate::session::build_conversation_key;
use crate::traits::{InboundReplyContext, OutboundContext};

/// Max visible chars pushed to an IM platform in one shot. Mirrors hermes
/// `MAX_PLATFORM_OUTPUT`. Oversized replies are truncated by
/// [`truncate_for_platform`] with a footer pointing to the run transcript.
pub const MAX_PLATFORM_OUTPUT: usize = 4000;

use std::sync::OnceLock;

use regex::Regex;

/// Hermes-aligned silence narration: whole-string only (length-guarded).
fn silence_narration_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^[\s*_~`]*\(?\s*(silent|silence|no\s+response|no\s+reply)\s*\.?\)?[\s*_~`]*$|^[\s*_~`]*[\x{1F507}\.\x{2026}\x{2025}…]+[\s*_~`]*$",
        )
        .expect("silence narration regex")
    })
}

/// Truncate `text` to [`MAX_PLATFORM_OUTPUT`] chars, appending a footer when
/// truncation occurs so the recipient knows the full output lives in the run
/// transcript (cron session / runs API).
pub fn truncate_for_platform(text: &str) -> String {
    if text.chars().count() <= MAX_PLATFORM_OUTPUT {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_PLATFORM_OUTPUT - 200).collect();
    format!(
        "{head}\n\n... [truncated, full output in the run transcript]"
    )
}

/// Return true when `content` is *only* a silence narration (hermes-aligned).
/// Covers `[SILENT]`, `silent` / `silence` / `no response` / `no reply` (with
/// optional markdown wrappers), 🔇, bare `.` / `…`. Substantive messages that
/// merely contain the word "silent" are never matched (anchored + length guard).
pub fn is_silence_narration(content: &str) -> bool {
    let stripped = content.trim();
    if stripped.is_empty() || stripped.chars().count() > 64 {
        return false;
    }
    if stripped.eq_ignore_ascii_case("[silent]") {
        return true;
    }
    silence_narration_re().is_match(stripped)
}

/// A selectable delivery target for settings UI / `GET .../delivery-targets`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTargetInfo {
    /// Spec to put in `deliver` (channel name for the happy path, e.g. `"feishu"`).
    pub deliver: String,
    pub channel: String,
    pub account_id: String,
    /// Short UI label (Chinese channel name + optional peer display name).
    pub label: String,
    pub recipient_id: String,
    pub is_group: bool,
    /// True when this comes from the account's configured home channel.
    pub is_home: bool,
    /// True when `homeRecipientId` is set (can be selected for cron deliver).
    pub bound: bool,
    /// Cached peer display name when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

fn channel_ui_name(channel: &str) -> String {
    match channel {
        "feishu" => "飞书".to_string(),
        "dingtalk" => "钉钉".to_string(),
        "wecom" => "企业微信".to_string(),
        "weixin" => "微信".to_string(),
        other => other.to_string(),
    }
}

/// List enabled channels as deliver options (one row per channel).
/// Unbound channels are included with `bound: false` so the UI can show a hint.
pub fn list_home_delivery_targets(cfg: &ChannelsConfig) -> Vec<DeliveryTargetInfo> {
    let mut out = Vec::new();
    for channel in ["feishu", "dingtalk", "wecom", "weixin"] {
        let accounts: Vec<_> = enabled_accounts(channel, cfg).collect();
        if accounts.is_empty() {
            continue;
        }
        // Prefer default account; else first enabled.
        let (account_id, account) = accounts
            .iter()
            .find(|(id, _)| id.as_str() == "default")
            .or_else(|| accounts.first())
            .map(|(id, a)| ((*id).clone(), (*a).clone()))
            .expect("accounts non-empty");
        let home = account.home_recipient_id.trim();
        let bound = !home.is_empty();
        let display_name = {
            let n = account.home_display_name.trim();
            if n.is_empty() {
                None
            } else {
                Some(n.to_string())
            }
        };
        let label = match (&display_name, bound) {
            (Some(name), true) => format!("{} · {name}", channel_ui_name(channel)),
            (_, true) if account.home_is_group => {
                format!("{} · 群", channel_ui_name(channel))
            }
            (_, true) => channel_ui_name(channel).to_string(),
            _ => format!("{}（未绑定）", channel_ui_name(channel)),
        };
        // Happy path: bare channel name resolves via home binding.
        let deliver = channel.to_string();
        out.push(DeliveryTargetInfo {
            deliver,
            channel: channel.to_string(),
            account_id,
            label,
            recipient_id: home.to_string(),
            is_group: account.home_is_group,
            is_home: bound,
            bound,
            display_name,
        });
    }
    out
}

/// Whether the channel has at least one enabled account with a non-empty home.
pub fn channel_is_bound(channel: &str, cfg: &ChannelsConfig) -> bool {
    resolve_home_channel(channel, cfg).is_some()
}

/// Normalize a user/tool deliver string for storage: trim, lowercase channel-name
/// tokens, drop empties. Returns `None` when the result is empty.
///
/// Explicit ID forms (`feishu:ou_xxx`) are preserved (recipient part unchanged);
/// bare channel names and `all` are lowercased.
pub fn normalize_deliver_spec(deliver: &str) -> Option<String> {
    let parts: Vec<String> = deliver
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|token| {
            if token.eq_ignore_ascii_case("all") {
                return "all".to_string();
            }
            if let Some((ch, rest)) = token.split_once(':') {
                let ch = ch.trim().to_ascii_lowercase();
                format!("{ch}:{}", rest.trim())
            } else {
                token.to_ascii_lowercase()
            }
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Validate that every channel named in `deliver` has a home binding.
/// Empty / `None` is ok (no IM push). Explicit `channel:id` forms skip the
/// binding check (advanced compatibility).
pub fn validate_deliver_spec(deliver: Option<&str>, cfg: &ChannelsConfig) -> Result<(), String> {
    let Some(raw) = deliver.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let Some(normalized) = normalize_deliver_spec(raw) else {
        return Ok(());
    };

    let mut saw_all = false;
    let mut named: Vec<&str> = Vec::new();
    for token in normalized.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if token == "all" {
            saw_all = true;
            continue;
        }
        if token.contains(':') {
            // Explicit target — do not require home binding.
            continue;
        }
        match token {
            "feishu" | "dingtalk" | "wecom" | "weixin" => named.push(token),
            other => {
                return Err(format!(
                    "Unknown deliver channel '{other}'. Use feishu, dingtalk, wecom, weixin, or all."
                ));
            }
        }
    }

    if saw_all {
        let any_bound = ["feishu", "dingtalk", "wecom", "weixin"]
            .iter()
            .any(|ch| channel_is_bound(ch, cfg));
        if !any_bound {
            return Err(
                "No IM channel is bound yet. Open Feishu/DingTalk/WeCom/Weixin and send Pointer a private message first."
                    .into(),
            );
        }
    }

    for ch in named {
        if !channel_is_bound(ch, cfg) {
            return Err(format!(
                "Channel '{ch}' has no deliver binding yet. Send Pointer a private message on that channel first, then retry."
            ));
        }
    }
    Ok(())
}

/// Parse a `deliver` spec into a list of outbound targets. Unknown channels /
/// accounts are skipped with a warning log (best-effort: one bad target does
/// not abort the others). Returns an empty vec when nothing resolves.
pub fn resolve_delivery_targets(deliver: &str, cfg: &ChannelsConfig) -> Vec<OutboundContext> {
    let deliver = deliver.trim();
    if deliver.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for token in deliver.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if token.eq_ignore_ascii_case("all") {
            for channel in ["feishu", "dingtalk", "wecom", "weixin"] {
                if let Some(ctx) = resolve_home_channel(channel, cfg) {
                    out.push(ctx);
                }
            }
            continue;
        }
        match parse_token(token) {
            Some(ParsedTarget::Home(channel)) => {
                if let Some(ctx) = resolve_home_channel(&channel, cfg) {
                    out.push(ctx);
                } else {
                    log::warn!(
                        "im_delivery: no home channel configured for {channel}; skipping"
                    );
                }
            }
            Some(ParsedTarget::Explicit {
                channel,
                recipient,
                is_group,
            }) => {
                if let Some(ctx) = resolve_explicit(&channel, &recipient, is_group, cfg) {
                    out.push(ctx);
                } else {
                    log::warn!(
                        "im_delivery: no enabled account for {channel}; skipping explicit target"
                    );
                }
            }
            None => {
                log::warn!("im_delivery: unparseable deliver token {token:?}; skipping");
            }
        }
    }
    out
}

enum ParsedTarget {
    /// `"<channel>"` — use the channel's home channel.
    Home(String),
    /// `"<channel>:<recipient>"` or `"<channel>:group:<chatid>"`.
    Explicit {
        channel: String,
        recipient: String,
        is_group: bool,
    },
}

fn parse_token(token: &str) -> Option<ParsedTarget> {
    let parts: Vec<&str> = token.split(':').collect();
    let channel = parts.first()?.trim().to_lowercase();
    if channel.is_empty() {
        return None;
    }
    if !matches!(channel.as_str(), "feishu" | "dingtalk" | "wecom" | "weixin") {
        return None;
    }
    if parts.len() == 1 {
        return Some(ParsedTarget::Home(channel));
    }
    if parts.len() == 3 && parts[1].trim().eq_ignore_ascii_case("group") {
        let recipient = parts[2].trim().to_string();
        if recipient.is_empty() {
            return None;
        }
        return Some(ParsedTarget::Explicit {
            channel,
            recipient,
            is_group: true,
        });
    }
    // `<channel>:<recipient>` — DM.
    let recipient = parts[1..].join(":").trim().to_string();
    if recipient.is_empty() {
        return None;
    }
    Some(ParsedTarget::Explicit {
        channel,
        recipient,
        is_group: false,
    })
}

/// Resolve a channel's home channel (first enabled account with
/// `home_recipient_id` set). Returns `None` if no account qualifies.
fn resolve_home_channel(channel: &str, cfg: &ChannelsConfig) -> Option<OutboundContext> {
    for (account_id, account) in enabled_accounts(channel, cfg) {
        let home = account.home_recipient_id.trim();
        if home.is_empty() {
            continue;
        }
        return Some(build_context(
            channel,
            account_id,
            home,
            account.home_is_group,
        ));
    }
    None
}

/// Resolve an explicit `"<channel>:<recipient>"` target. Picks the first
/// enabled account on the channel (the recipient id is platform-global, so
/// any enabled account's credentials can address it).
fn resolve_explicit(
    channel: &str,
    recipient: &str,
    is_group: bool,
    cfg: &ChannelsConfig,
) -> Option<OutboundContext> {
    let (account_id, _) = enabled_accounts(channel, cfg).next()?;
    Some(build_context(channel, account_id, recipient, is_group))
}

/// Iterate enabled accounts on a channel as `(account_id, config)` pairs.
fn enabled_accounts<'a>(
    channel: &str,
    cfg: &'a ChannelsConfig,
) -> impl Iterator<Item = (&'a String, &'a crate::config::ChannelAccountConfig)> {
    let iter: Box<dyn Iterator<Item = (&'a String, &'a crate::config::ChannelAccountConfig)>> =
        match channel {
            "feishu" => Box::new(cfg.feishu.iter()),
            "dingtalk" => Box::new(cfg.dingtalk.iter()),
            "wecom" => Box::new(cfg.wecom.iter()),
            "weixin" => Box::new(cfg.weixin.iter()),
            _ => Box::new(std::iter::empty()),
        };
    iter.filter(|(_, a)| a.enabled)
}

/// Build an [`OutboundContext`] for a channel + recipient.
///
/// Channel-specific quirks:
/// - DingTalk group: the outbound adapter reads `openConversationId` from
///   `reply_context.chat_id` (not `recipient_id`) for `groupMessages/send`, so
///   we mirror it there.
/// - WeCom group: WSS `send_markdown` prefers `reply_context.chat_id`; mirror
///   it so WSS-connected accounts push to the right chat.
/// - Feishu: `receive_target` auto-detects `chat_id` (oc_ prefix) vs `open_id`
///   from `recipient_id`, so no reply_context is needed.
/// - Weixin: relies on a cached `context_token` for the recipient; no
///   reply_context is needed here (the adapter resolves the token itself).
fn build_context(channel: &str, account_id: &str, recipient: &str, is_group: bool) -> OutboundContext {
    let conversation_key = build_conversation_key(channel, recipient, is_group);
    let reply_context = if is_group && matches!(channel, "dingtalk" | "wecom") {
        Some(InboundReplyContext {
            chat_id: Some(recipient.to_string()),
            ..Default::default()
        })
    } else {
        None
    };
    OutboundContext {
        channel: channel.to_string(),
        account_id: account_id.to_string(),
        conversation_key,
        recipient_id: recipient.to_string(),
        reply_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChannelAccountConfig, ChannelsConfig};
    use std::collections::HashMap;

    fn account(home: &str, is_group: bool) -> ChannelAccountConfig {
        let mut a = ChannelAccountConfig::default();
        a.enabled = true;
        a.home_recipient_id = home.to_string();
        a.home_is_group = is_group;
        a
    }

    fn cfg_with_feishu(home: &str, is_group: bool) -> ChannelsConfig {
        let mut cfg = ChannelsConfig::default();
        let mut m = HashMap::new();
        m.insert("default".to_string(), account(home, is_group));
        cfg.feishu = m;
        cfg
    }

    #[test]
    fn empty_deliver_returns_empty() {
        let cfg = ChannelsConfig::default();
        assert!(resolve_delivery_targets("", &cfg).is_empty());
        assert!(resolve_delivery_targets("   ", &cfg).is_empty());
    }

    #[test]
    fn feishu_home_channel() {
        let cfg = cfg_with_feishu("oc_123", true);
        let out = resolve_delivery_targets("feishu", &cfg);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel, "feishu");
        assert_eq!(out[0].account_id, "default");
        assert_eq!(out[0].recipient_id, "oc_123");
        assert_eq!(out[0].conversation_key, "feishu:group:oc_123");
        // Feishu group does not need reply_context (receive_target auto-detects).
        assert!(out[0].reply_context.is_none());
    }

    #[test]
    fn feishu_explicit_dm() {
        let cfg = cfg_with_feishu("oc_home", true);
        let out = resolve_delivery_targets("feishu:ou_user1", &cfg);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].recipient_id, "ou_user1");
        assert_eq!(out[0].conversation_key, "feishu:dm:ou_user1");
        assert!(out[0].reply_context.is_none());
    }

    #[test]
    fn feishu_explicit_group() {
        let cfg = cfg_with_feishu("oc_home", true);
        let out = resolve_delivery_targets("feishu:group:oc_456", &cfg);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].recipient_id, "oc_456");
        assert_eq!(out[0].conversation_key, "feishu:group:oc_456");
        assert!(out[0].reply_context.is_none());
    }

    #[test]
    fn dingtalk_group_mirrors_chat_id_into_reply_context() {
        let cfg = ChannelsConfig::default();
        let out = resolve_delivery_targets("dingtalk:group:cid_conv1", &cfg);
        // No enabled dingtalk account configured → skipped.
        assert!(out.is_empty());

        let mut cfg = cfg;
        let mut m = HashMap::new();
        m.insert("default".to_string(), account("", false));
        cfg.dingtalk = m;
        let out = resolve_delivery_targets("dingtalk:group:cid_conv1", &cfg);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].recipient_id, "cid_conv1");
        assert_eq!(out[0].conversation_key, "dingtalk:group:cid_conv1");
        let rc = out[0].reply_context.as_ref().unwrap();
        assert_eq!(rc.chat_id.as_deref(), Some("cid_conv1"));
    }

    #[test]
    fn all_targets_iterates_every_channel_with_home() {
        let mut cfg = ChannelsConfig::default();
        let mut f = HashMap::new();
        f.insert("default".to_string(), account("oc_f", true));
        cfg.feishu = f;
        let mut d = HashMap::new();
        d.insert("default".to_string(), account("uid_d", false));
        cfg.dingtalk = d;
        // wecom / weixin have no accounts → skipped.
        let out = resolve_delivery_targets("all", &cfg);
        assert_eq!(out.len(), 2);
        let channels: Vec<&str> = out.iter().map(|c| c.channel.as_str()).collect();
        assert!(channels.contains(&"feishu"));
        assert!(channels.contains(&"dingtalk"));
    }

    #[test]
    fn comma_separated_multi_target() {
        let cfg = cfg_with_feishu("oc_home", true);
        let out = resolve_delivery_targets("feishu:ou_a,feishu:group:oc_b", &cfg);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].recipient_id, "ou_a");
        assert_eq!(out[1].recipient_id, "oc_b");
    }

    #[test]
    fn unknown_channel_skipped() {
        let cfg = ChannelsConfig::default();
        let out = resolve_delivery_targets("slack:chan1", &cfg);
        assert!(out.is_empty());
    }

    #[test]
    fn silence_narration_detection() {
        assert!(is_silence_narration("[SILENT]"));
        assert!(is_silence_narration("  [silent]  "));
        assert!(is_silence_narration("silent"));
        assert!(is_silence_narration("*(silence)*"));
        assert!(is_silence_narration("no response"));
        assert!(is_silence_narration("."));
        assert!(is_silence_narration("…"));
        assert!(!is_silence_narration("[SILENT] some content"));
        assert!(!is_silence_narration("The deployment ran silently"));
        assert!(!is_silence_narration("hello"));
    }

    #[test]
    fn list_home_targets_from_config() {
        let cfg = cfg_with_feishu("ou_home", false);
        let list = list_home_delivery_targets(&cfg);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].deliver, "feishu");
        assert!(list[0].is_home);
        assert!(list[0].bound);
        assert!(!list[0].is_group);
        assert_eq!(list[0].label, "飞书");
    }

    #[test]
    fn list_includes_unbound_enabled_channel() {
        let mut cfg = ChannelsConfig::default();
        let mut m = HashMap::new();
        m.insert("default".to_string(), account("", false));
        cfg.feishu = m;
        let list = list_home_delivery_targets(&cfg);
        assert_eq!(list.len(), 1);
        assert!(!list[0].bound);
        assert!(list[0].label.contains("未绑定"));
    }

    #[test]
    fn validate_deliver_requires_binding() {
        let cfg = ChannelsConfig::default();
        assert!(validate_deliver_spec(None, &cfg).is_ok());
        assert!(validate_deliver_spec(Some(""), &cfg).is_ok());
        assert!(validate_deliver_spec(Some("feishu"), &cfg).is_err());
        assert!(validate_deliver_spec(Some("all"), &cfg).is_err());

        let cfg = cfg_with_feishu("ou_1", false);
        assert!(validate_deliver_spec(Some("feishu"), &cfg).is_ok());
        assert!(validate_deliver_spec(Some("Feishu"), &cfg).is_ok());
        assert!(validate_deliver_spec(Some("all"), &cfg).is_ok());
        assert!(validate_deliver_spec(Some("dingtalk"), &cfg).is_err());
        // Explicit ID skips binding check.
        assert!(validate_deliver_spec(Some("feishu:ou_x"), &ChannelsConfig::default()).is_ok());
    }

    #[test]
    fn normalize_deliver_lowercases_channel_names() {
        assert_eq!(
            normalize_deliver_spec("Feishu, DingTalk").as_deref(),
            Some("feishu,dingtalk")
        );
        assert_eq!(normalize_deliver_spec("ALL").as_deref(), Some("all"));
        assert_eq!(
            normalize_deliver_spec("feishu:OU_x").as_deref(),
            Some("feishu:OU_x")
        );
    }

    #[test]
    fn truncate_keeps_short_text_intact() {
        assert_eq!(truncate_for_platform("hi"), "hi");
    }

    #[test]
    fn truncate_adds_footer_for_long_text() {
        let long = "a".repeat(MAX_PLATFORM_OUTPUT + 500);
        let out = truncate_for_platform(&long);
        assert!(out.ends_with("... [truncated, full output in the run transcript]"));
        assert!(out.chars().count() <= MAX_PLATFORM_OUTPUT + 100);
    }
}
