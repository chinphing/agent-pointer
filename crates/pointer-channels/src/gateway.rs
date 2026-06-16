use anyhow::Result;
use parking_lot::RwLock;
use pointer_core::chat_service::AppState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::config::{load_channels_config, ChannelsConfig};
use crate::dedup::DedupStore;
use crate::dispatch::DispatchService;
use crate::pairing::PairingStore;
use crate::registry::ChannelRegistry;
use crate::session::conversation_id;
use crate::session_abort::{is_abort_command, ABORT_ACK};
use crate::traits::{InboundMessage, OutboundContext};

pub struct ChannelGateway {
    registry: ChannelRegistry,
    config: RwLock<ChannelsConfig>,
    dispatch: DispatchService,
    pub pairing: PairingStore,
    pub dedup: DedupStore,
    core: Arc<AppState>,
    /// Latest Weixin `context_token` per (account_id, sender_id) for outbound replies.
    weixin_context_tokens: RwLock<HashMap<String, HashMap<String, String>>>,
}

impl ChannelGateway {
    pub fn new(core: Arc<AppState>, registry: ChannelRegistry) -> Result<Self> {
        let config = load_channels_config().unwrap_or_default();
        Ok(Self {
            registry,
            config: RwLock::new(config),
            dispatch: DispatchService::new()?,
            pairing: PairingStore::new(),
            dedup: DedupStore::new(),
            core,
            weixin_context_tokens: RwLock::new(HashMap::new()),
        })
    }

    pub fn registry(&self) -> &ChannelRegistry {
        &self.registry
    }

    pub fn config(&self) -> parking_lot::RwLockReadGuard<'_, ChannelsConfig> {
        self.config.read()
    }

    pub fn update_config(&self, cfg: ChannelsConfig) -> Result<()> {
        crate::config::save_channels_config(&cfg)?;
        *self.config.write() = cfg;
        Ok(())
    }

    pub fn reload_config(&self) -> Result<()> {
        *self.config.write() = load_channels_config()?;
        Ok(())
    }

    /// Manual / API outbound (OpenClaw `openclaw message send --media`).
    pub async fn send_outbound_explicit(
        &self,
        ctx: &OutboundContext,
        text: Option<&str>,
        media_paths: &[String],
    ) -> Result<()> {
        crate::outbound_delivery::deliver_outbound_explicit(self, ctx.clone(), text, media_paths)
            .await
    }

    pub async fn process_inbound(&self, msg: InboundMessage) -> Result<()> {
        let namespace = format!("{}:{}", msg.channel, msg.account_id);
        if self.dedup.is_seen(&namespace, &msg.dedup_key()) {
            log::info!("channel dedup drop id={}", msg.dedup_key());
            return Ok(());
        }

        let cfg = self.config.read().clone();
        let account = cfg
            .account(&msg.channel, &msg.account_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("account not configured"))?;
        if !account.enabled {
            return Ok(());
        }

        let _ = self.pairing.load(&msg.channel, &msg.account_id);

        let code_candidate = msg.text.trim();
        if code_candidate.len() == 8
            && code_candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric())
        {
            if self
                .pairing
                .approve(&msg.channel, &msg.account_id, code_candidate)?
            {
                let plugin = self
                    .registry
                    .get(&msg.channel)
                    .ok_or_else(|| anyhow::anyhow!("plugin missing"))?;
                let outbound = crate::traits::OutboundContext {
                    channel: msg.channel.clone(),
                    account_id: msg.account_id.clone(),
                    conversation_key: msg.conversation_key.clone(),
                    recipient_id: msg.sender_id.clone(),
                    reply_context: msg.reply_context.clone(),
                };
                plugin
                    .outbound
                    .send_text(outbound, "配对成功！请重新发送您的消息。")
                    .await?;
                self.dedup.mark_seen(&namespace, &msg.dedup_key());
                return Ok(());
            }
        }

        match self.pairing.is_allowed(
            &msg.channel,
            &msg.account_id,
            &msg.sender_id,
            &account.dm_policy,
            &account.allow_from,
        ) {
            crate::pairing::PairingDecision::Deny => {
                log::info!("channel dm denied sender={}", msg.sender_id);
                return Ok(());
            }
            crate::pairing::PairingDecision::NeedPairing => {
                let code = self
                    .pairing
                    .issue_code(&msg.channel, &msg.account_id, &msg.sender_id);
                let plugin = self
                    .registry
                    .get(&msg.channel)
                    .ok_or_else(|| anyhow::anyhow!("plugin missing"))?;
                let outbound = crate::traits::OutboundContext {
                    channel: msg.channel.clone(),
                    account_id: msg.account_id.clone(),
                    conversation_key: msg.conversation_key.clone(),
                    recipient_id: msg.sender_id.clone(),
                    reply_context: msg.reply_context.clone(),
                };
                let approve_label = match msg.channel.as_str() {
                    "weixin" => "微信批准",
                    "wecom" => "企微批准",
                    "feishu" => "飞书批准",
                    "dingtalk" => "钉钉批准",
                    _ => "对应通道批准",
                };
                plugin
                    .outbound
                    .send_text(
                        outbound,
                        &format!(
                            "需要配对。请在 Pointer 设置 → IM 通道 → 配对审批 中点击「{approve_label}」，或直接回复此配对码：{code}"
                        ),
                    )
                    .await?;
                self.dedup.mark_seen(&namespace, &msg.dedup_key());
                return Ok(());
            }
            crate::pairing::PairingDecision::Allow => {}
        }

        let plugin = self
            .registry
            .get(&msg.channel)
            .ok_or_else(|| anyhow::anyhow!("plugin missing"))?;

        let mut msg = msg;
        self.enrich_weixin_reply_context(&mut msg);

        let conv_id = conversation_id(&msg);
        if is_abort_command(&msg.text) {
            self.core.cancel(&conv_id);
            let outbound = OutboundContext {
                channel: msg.channel.clone(),
                account_id: msg.account_id.clone(),
                conversation_key: msg.conversation_key.clone(),
                recipient_id: msg.sender_id.clone(),
                reply_context: msg.reply_context.clone(),
            };
            plugin.outbound.send_text(outbound, ABORT_ACK).await?;
            if let Err(e) = self.core.session_index.touch_im_interaction(&conv_id) {
                log::warn!("channel abort touch interaction failed conv={conv_id}: {e:#}");
            }
            self.dedup.mark_seen(&namespace, &msg.dedup_key());
            log::info!("channel fast abort conv={conv_id}");
            return Ok(());
        }

        let dedup_key = msg.dedup_key();
        let idle_minutes = cfg.meta.session_reset.effective_idle_minutes();
        match self
            .dispatch
            .handle_inbound(
                self.core.clone(),
                &plugin,
                &account,
                idle_minutes,
                msg,
            )
            .await
        {
            Ok(()) => {
                self.dedup.mark_seen(&namespace, &dedup_key);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn spawn_weixin_monitors(self: &Arc<Self>, cancel: CancellationToken) {
        let cfg = self.config.read().clone();
        for (account_id, account) in cfg.weixin.iter() {
            if !account.enabled {
                continue;
            }
            let gw = self.clone();
            let account_id = account_id.clone();
            let cancel_child = cancel.child_token();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::adapters::weixin::monitor::run_weixin_monitor(gw, account_id, cancel_child)
                        .await
                {
                    log::error!("weixin monitor stopped: {e:#}");
                }
            });
        }
    }

    pub fn spawn_wecom_monitors(self: &Arc<Self>, cancel: CancellationToken) {
        let cfg = self.config.read().clone();
        let mut started = 0u32;
        for (account_id, account) in cfg.wecom.iter() {
            if !account.enabled || account.connection_mode != "websocket" {
                log::info!(
                    "wecom ws monitor skipped account={account_id} enabled={} mode={}",
                    account.enabled,
                    account.connection_mode
                );
                continue;
            }
            if account.bot_id.trim().is_empty() || account.secret.trim().is_empty() {
                log::warn!(
                    "wecom ws monitor skipped account={account_id}: missing botId or secret"
                );
                continue;
            }
            started += 1;
            log::info!("wecom ws monitor starting account={account_id}");
            let gw = self.clone();
            let account_id = account_id.clone();
            let cancel_child = cancel.child_token();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::adapters::wecom::monitor::run_wecom_monitor(gw, account_id, cancel_child)
                        .await
                {
                    log::error!("wecom ws monitor stopped: {e:#}");
                }
            });
        }
        if started == 0 {
            log::info!("wecom ws monitor: no websocket accounts to start");
        }
    }

    pub fn spawn_feishu_monitors(self: &Arc<Self>, cancel: CancellationToken) {
        let cfg = self.config.read().clone();
        let mut started = 0u32;
        for (account_id, account) in cfg.feishu.iter() {
            if !account.enabled || account.connection_mode != "websocket" {
                continue;
            }
            if account.app_id.trim().is_empty() || account.app_secret.trim().is_empty() {
                log::warn!(
                    "feishu ws monitor skipped account={account_id}: missing appId or appSecret"
                );
                continue;
            }
            started += 1;
            log::info!("feishu ws monitor starting account={account_id}");
            let gw = self.clone();
            let account_id = account_id.clone();
            let cancel_child = cancel.child_token();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::adapters::feishu::run_feishu_monitor(gw, account_id, cancel_child).await
                {
                    log::error!("feishu ws monitor stopped: {e:#}");
                }
            });
        }
        if started == 0 {
            log::info!("feishu ws monitor: no websocket accounts to start");
        }
    }

    pub fn spawn_dingtalk_monitors(self: &Arc<Self>, cancel: CancellationToken) {
        let cfg = self.config.read().clone();
        let mut started = 0u32;
        for (account_id, account) in cfg.dingtalk.iter() {
            if !account.enabled || account.connection_mode != "websocket" {
                continue;
            }
            if account.client_id.trim().is_empty() || account.client_secret.trim().is_empty() {
                log::warn!(
                    "dingtalk stream monitor skipped account={account_id}: missing clientId or clientSecret"
                );
                continue;
            }
            started += 1;
            log::info!("dingtalk stream monitor starting account={account_id}");
            let gw = self.clone();
            let account_id = account_id.clone();
            let cancel_child = cancel.child_token();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::adapters::dingtalk::run_dingtalk_monitor(gw, account_id, cancel_child)
                        .await
                {
                    log::error!("dingtalk stream monitor stopped: {e:#}");
                }
            });
        }
        if started == 0 {
            log::info!("dingtalk stream monitor: no websocket accounts to start");
        }
    }

    fn enrich_weixin_reply_context(&self, msg: &mut InboundMessage) {
        if msg.channel != "weixin" {
            return;
        }
        if let Some(token) = msg
            .reply_context
            .as_ref()
            .and_then(|r| r.context_token.as_deref())
            .filter(|s| !s.trim().is_empty())
        {
            self.weixin_context_tokens
                .write()
                .entry(msg.account_id.clone())
                .or_default()
                .insert(msg.sender_id.clone(), token.to_string());
            return;
        }
        let stored = self
            .weixin_context_tokens
            .read()
            .get(&msg.account_id)
            .and_then(|m| m.get(&msg.sender_id).cloned());
        let Some(token) = stored else {
            return;
        };
        match msg.reply_context.as_mut() {
            Some(ctx) => ctx.context_token = Some(token),
            None => {
                msg.reply_context = Some(crate::traits::InboundReplyContext {
                    session_webhook: None,
                    chat_id: None,
                    open_id: Some(msg.sender_id.clone()),
                    context_token: Some(token),
                    wecom_req_id: None,
                });
            }
        }
    }
}
