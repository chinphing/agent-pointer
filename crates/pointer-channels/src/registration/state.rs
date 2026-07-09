use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::gateway::ChannelGateway;
use crate::monitor_supervisor::MonitorSupervisor;

use super::dingtalk;
use super::feishu;
use super::qr::qrcode_png_base64;
use super::wecom;

#[derive(Clone)]
pub struct RegistrationCompletion {
    gateway: Arc<ChannelGateway>,
    monitors: Arc<MonitorSupervisor>,
}

impl RegistrationCompletion {
    pub fn new(gateway: Arc<ChannelGateway>, monitors: Arc<MonitorSupervisor>) -> Self {
        Self { gateway, monitors }
    }

    fn on_success(&self, channel: &str, account_id: &str, session: &RegistrationSession) {
        if let Err(e) = self.gateway.persist_registration_and_restart(
            channel,
            account_id,
            session,
            self.monitors.as_ref(),
        ) {
            log::error!(
                "channel registration persist/restart failed channel={channel} account={account_id}: {e:#}"
            );
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationSession {
    pub channel: String,
    pub account_id: String,
    pub qr_url: String,
    pub qrcode_png_base64: String,
    pub status: String,
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub bot_id: Option<String>,
    pub secret: Option<String>,
    pub error_message: Option<String>,
}

struct PollContext {
    channel: String,
    device_code: String,
    interval_secs: u64,
    expire_secs: u64,
}

pub struct ChannelRegistrationState {
    sessions: Arc<RwLock<HashMap<String, RegistrationSession>>>,
    completion: Option<RegistrationCompletion>,
}

impl ChannelRegistrationState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            completion: None,
        }
    }

    pub fn with_completion(gateway: Arc<ChannelGateway>, monitors: Arc<MonitorSupervisor>) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            completion: Some(RegistrationCompletion::new(gateway, monitors)),
        }
    }

    pub async fn start(&self, channel: &str, account_id: &str) -> Result<RegistrationSession> {
        let key = session_key(channel, account_id);
        let (qr_url, poll_ctx) = match channel {
            "feishu" => {
                let begin = feishu::begin_registration().await?;
                (
                    begin.qr_url.clone(),
                    PollContext {
                        channel: channel.into(),
                        device_code: begin.device_code,
                        interval_secs: begin.interval_secs,
                        expire_secs: begin.expire_secs,
                    },
                )
            }
            "dingtalk" => {
                let begin = dingtalk::begin_registration().await?;
                (
                    begin.qr_url.clone(),
                    PollContext {
                        channel: channel.into(),
                        device_code: begin.device_code,
                        interval_secs: begin.interval_secs,
                        expire_secs: begin.expire_secs,
                    },
                )
            }
            "wecom" => {
                let begin = wecom::begin_registration().await?;
                (
                    begin.qr_url.clone(),
                    PollContext {
                        channel: channel.into(),
                        device_code: begin.scode,
                        interval_secs: begin.interval_secs,
                        expire_secs: begin.expire_secs,
                    },
                )
            }
            other => anyhow::bail!("channel {other} does not support QR registration"),
        };

        let png_b64 = qrcode_png_base64(&qr_url)?;
        let session = RegistrationSession {
            channel: channel.into(),
            account_id: account_id.into(),
            qr_url,
            qrcode_png_base64: png_b64,
            status: "pending".into(),
            app_id: None,
            app_secret: None,
            client_id: None,
            client_secret: None,
            bot_id: None,
            secret: None,
            error_message: None,
        };

        self.sessions
            .write()
            .await
            .insert(key.clone(), session.clone());

        let sessions = self.sessions.clone();
        let completion = self.completion.clone();
        tokio::spawn(async move {
            if let Err(e) = poll_until_done(sessions, &key, poll_ctx, completion).await {
                log::error!("channel registration poll failed key={key}: {e:#}");
            }
        });

        log::info!("channel registration started channel={channel} account={account_id}");
        Ok(session)
    }

    pub async fn get(&self, channel: &str, account_id: &str) -> Option<RegistrationSession> {
        let key = session_key(channel, account_id);
        self.sessions.read().await.get(&key).cloned()
    }
}

async fn poll_until_done(
    sessions: Arc<RwLock<HashMap<String, RegistrationSession>>>,
    key: &str,
    ctx: PollContext,
    completion: Option<RegistrationCompletion>,
) -> Result<()> {
    let (channel, account_id) = parse_session_key(key)
        .ok_or_else(|| anyhow::anyhow!("invalid registration session key {key}"))?;

    let poll_result: Result<()> = match ctx.channel.as_str() {
        "feishu" => {
            match feishu::poll_registration(
                &ctx.device_code,
                ctx.interval_secs,
                ctx.expire_secs,
            )
            .await
            {
                Ok(Some(creds)) => {
                    let session_snapshot = {
                        let mut guard = sessions.write().await;
                        if let Some(session) = guard.get_mut(key) {
                            session.app_id = Some(creds.app_id);
                            session.app_secret = Some(creds.app_secret);
                            session.status = "success".into();
                            session.clone()
                        } else {
                            return Ok(());
                        }
                    };
                    if let Some(c) = &completion {
                        c.on_success(channel, account_id, &session_snapshot);
                    }
                    log::info!("feishu registration success key={key}");
                    Ok(())
                }
                Ok(None) => Err(anyhow::anyhow!("授权未完成")),
                Err(e) => Err(e),
            }
        }
        "dingtalk" => {
            match dingtalk::poll_registration(
                &ctx.device_code,
                ctx.interval_secs,
                ctx.expire_secs,
            )
            .await
            {
                Ok(Some(creds)) => {
                    let session_snapshot = {
                        let mut guard = sessions.write().await;
                        if let Some(session) = guard.get_mut(key) {
                            session.client_id = Some(creds.client_id);
                            session.client_secret = Some(creds.client_secret);
                            session.status = "success".into();
                            session.clone()
                        } else {
                            return Ok(());
                        }
                    };
                    if let Some(c) = &completion {
                        c.on_success(channel, account_id, &session_snapshot);
                    }
                    log::info!("dingtalk registration success key={key}");
                    Ok(())
                }
                Ok(None) => Err(anyhow::anyhow!("授权未完成")),
                Err(e) => Err(e),
            }
        }
        "wecom" => {
            match wecom::poll_registration(
                &ctx.device_code,
                ctx.interval_secs,
                ctx.expire_secs,
            )
            .await
            {
                Ok(Some(creds)) => {
                    let session_snapshot = {
                        let mut guard = sessions.write().await;
                        if let Some(session) = guard.get_mut(key) {
                            session.bot_id = Some(creds.bot_id);
                            session.secret = Some(creds.secret);
                            session.status = "success".into();
                            session.clone()
                        } else {
                            return Ok(());
                        }
                    };
                    if let Some(c) = &completion {
                        c.on_success(channel, account_id, &session_snapshot);
                    }
                    log::info!("wecom registration success key={key}");
                    Ok(())
                }
                Ok(None) => Err(anyhow::anyhow!("授权未完成")),
                Err(e) => Err(e),
            }
        }
        other => Err(anyhow::anyhow!("unsupported channel {other}")),
    };

    if let Err(e) = poll_result {
        let msg = format!("{e:#}");
        let mut guard = sessions.write().await;
        if let Some(session) = guard.get_mut(key) {
            session.status = if msg.contains("过期") {
                "expired".into()
            } else if msg.contains("拒绝") {
                "denied".into()
            } else if msg.contains("超时") {
                "timeout".into()
            } else {
                "failed".into()
            };
            session.error_message = Some(msg);
        }
    }
    Ok(())
}

fn session_key(channel: &str, account_id: &str) -> String {
    format!("{channel}:{account_id}")
}

fn parse_session_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}
