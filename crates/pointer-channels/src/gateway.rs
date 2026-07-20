use anyhow::Result;
use parking_lot::RwLock;
use pointer_core::chat_service::AppState;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::adapters::weixin::context_token;
use crate::config::{load_channels_config, save_channels_config, ChannelsConfig};
use crate::monitor_supervisor::MonitorSupervisor;
use crate::registration::{apply_registration_session_to_config, RegistrationSession};
use crate::dedup::DedupStore;
use crate::dispatch::DispatchService;
use crate::pairing::PairingStore;
use crate::registry::ChannelRegistry;
use crate::session::conversation_id;
use crate::session_abort::{is_abort_command, ABORT_ACK};
use crate::session_about::{is_about_command, about_text};
use crate::traits::{InboundMessage, OutboundContext};

pub struct ChannelGateway {
    registry: ChannelRegistry,
    config: RwLock<ChannelsConfig>,
    dispatch: DispatchService,
    pub pairing: PairingStore,
    pub dedup: DedupStore,
    core: Arc<AppState>,
}

impl ChannelGateway {
    pub fn new(core: Arc<AppState>, registry: ChannelRegistry) -> Result<Self> {
        pointer_core::tls::ensure_rustls_crypto_provider();
        let config = load_channels_config().unwrap_or_default();
        let pairing = PairingStore::new();
        pairing.load_all()?;
        Ok(Self {
            registry,
            config: RwLock::new(config),
            dispatch: DispatchService::new()?,
            pairing,
            dedup: DedupStore::new(),
            core,
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

    /// Persist the channel account's deliver binding from an inbound **DM**.
    /// Last DM wins (overwrites prior binding). Group chats must not call this.
    /// Returns `Ok(true)` when the stored binding changed.
    pub fn set_home_binding_from_dm(
        &self,
        channel: &str,
        account_id: &str,
        recipient_id: &str,
        display_name: Option<&str>,
    ) -> Result<bool> {
        let recipient_id = recipient_id.trim();
        if recipient_id.is_empty() || recipient_id.eq_ignore_ascii_case("unknown") {
            return Ok(false);
        }
        let display_name = display_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");

        let mut cfg = self.config.read().clone();
        let Some(account) = cfg.account_mut(channel, account_id) else {
            log::warn!(
                "im binding: skip unknown account channel={channel} account={account_id}"
            );
            return Ok(false);
        };

        let same_recipient = account.home_recipient_id.trim() == recipient_id
            && !account.home_is_group;
        let same_name = display_name.is_empty()
            || account.home_display_name.trim() == display_name;
        if same_recipient && same_name {
            return Ok(false);
        }

        account.home_recipient_id = recipient_id.to_string();
        account.home_is_group = false;
        if !display_name.is_empty() {
            account.home_display_name = display_name.to_string();
        }
        self.update_config(cfg)?;
        log::info!(
            "im binding: channel={channel} account={account_id} recipient={recipient_id} display_name={}",
            if display_name.is_empty() {
                "(unchanged/empty)"
            } else {
                display_name
            }
        );
        Ok(true)
    }

    pub fn update_config_and_restart(
        self: &Arc<Self>,
        cfg: ChannelsConfig,
        supervisor: &MonitorSupervisor,
    ) -> Result<()> {
        save_channels_config(&cfg)?;
        *self.config.write() = cfg;
        supervisor.restart(Arc::clone(self));
        log::info!("channel config updated and monitors restarted");
        Ok(())
    }

    pub fn reload_config(&self) -> Result<()> {
        *self.config.write() = load_channels_config()?;
        Ok(())
    }

    /// Persist QR registration credentials, refresh in-memory config, and restart monitors.
    pub fn persist_registration_and_restart(
        self: &Arc<Self>,
        channel: &str,
        account_id: &str,
        session: &RegistrationSession,
        supervisor: &MonitorSupervisor,
    ) -> Result<()> {
        let mut cfg = load_channels_config()?;
        if !apply_registration_session_to_config(&mut cfg, channel, account_id, session)? {
            log::warn!(
                "channel registration persist skipped channel={channel} account={account_id}: missing credentials"
            );
            return Ok(());
        }
        crate::config::save_channels_config(&cfg)?;
        *self.config.write() = cfg;
        supervisor.restart(Arc::clone(self));
        log::info!(
            "channel registration persisted and monitors restarted channel={channel} account={account_id}"
        );
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

        // Auto-bind last DM peer as this account's deliver home (product: one
        // binding per channel). Groups never update the binding.
        if !msg.is_group {
            if let Err(e) = self.set_home_binding_from_dm(
                &msg.channel,
                &msg.account_id,
                &msg.sender_id,
                msg.sender_name.as_deref(),
            ) {
                log::warn!(
                    "im binding: failed channel={} account={} sender={}: {e:#}",
                    msg.channel,
                    msg.account_id,
                    msg.sender_id
                );
            }
        }

        let plugin = self
            .registry
            .get(&msg.channel)
            .ok_or_else(|| anyhow::anyhow!("plugin missing"))?;

        let mut msg = msg;
        self.enrich_weixin_reply_context(&mut msg);

        if is_about_command(&msg.text) {
            let outbound = OutboundContext {
                channel: msg.channel.clone(),
                account_id: msg.account_id.clone(),
                conversation_key: msg.conversation_key.clone(),
                recipient_id: msg.sender_id.clone(),
                reply_context: msg.reply_context.clone(),
            };
            plugin
                .outbound
                .send_text(outbound, &about_text())
                .await?;
            self.dedup.mark_seen(&namespace, &msg.dedup_key());
            return Ok(());
        }

        let conv_id = conversation_id(&msg, Some(&account.dynamic_agents));
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
            context_token::set(&msg.account_id, &msg.sender_id, token);
            return;
        }
        let stored = context_token::get(&msg.account_id, &msg.sender_id);
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
