use anyhow::Result;
use parking_lot::RwLock;
use pointer_core::chat_service::AppState;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::config::{load_channels_config, ChannelsConfig};
use crate::dedup::DedupStore;
use crate::dispatch::DispatchService;
use crate::pairing::PairingStore;
use crate::registry::ChannelRegistry;
use crate::traits::InboundMessage;

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
        let config = load_channels_config().unwrap_or_default();
        Ok(Self {
            registry,
            config: RwLock::new(config),
            dispatch: DispatchService::new(),
            pairing: PairingStore::new(),
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

    pub fn reload_config(&self) -> Result<()> {
        *self.config.write() = load_channels_config()?;
        Ok(())
    }

    pub async fn process_inbound(&self, msg: InboundMessage) -> Result<()> {
        let namespace = format!("{}:{}", msg.channel, msg.account_id);
        if self.dedup.seen(&namespace, &msg.message_id) {
            log::info!("channel dedup drop id={}", msg.message_id);
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
                plugin
                    .outbound
                    .send_text(
                        outbound,
                        &format!(
                            "需要配对。请在 Pointer 设置 → IM 通道 → 配对审批 中点击「企微批准」，或直接回复此配对码：{code}"
                        ),
                    )
                    .await?;
                return Ok(());
            }
            crate::pairing::PairingDecision::Allow => {}
        }

        let plugin = self
            .registry
            .get(&msg.channel)
            .ok_or_else(|| anyhow::anyhow!("plugin missing"))?;
        self.dispatch
            .handle_inbound(self.core.clone(), &plugin, &account, msg)
            .await
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
}
