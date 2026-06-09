use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use crate::connection_state;
use crate::gateway::ChannelGateway;
use crate::http_client::HttpClient;
use crate::traits::ChannelWebhookAdapter;
const OPEN_URL: &str = "https://api.dingtalk.com/v1.0/gateway/connections/open";
const BOT_MSG_TOPIC: &str = "/v1.0/im/bot/messages/get";
const HEARTBEAT_INTERVAL_MS: u64 = 30_000;
const INBOUND_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const RECONNECT_BASE_MS: u64 = 1_000;
const RECONNECT_MAX_MS: u64 = 30_000;

pub struct DingTalkStreamConfig {
    pub account_id: String,
    pub client_id: String,
    pub client_secret: String,
}

pub async fn run_dingtalk_stream_loop(
    cfg: DingTalkStreamConfig,
    gateway: std::sync::Arc<ChannelGateway>,
    cancel: CancellationToken,
) -> Result<()> {
    let http = HttpClient::new()?;
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            log::info!("dingtalk stream loop cancelled account={}", cfg.account_id);
            return Ok(());
        }
        match run_single_connection(&cfg, &http, gateway.clone(), cancel.clone(), &mut attempt)
            .await
        {
            Ok(StopReason::Cancelled) => return Ok(()),
            Ok(StopReason::Disconnected) => {
                attempt = attempt.saturating_add(1);
                let delay = reconnect_delay(attempt);
                log::warn!(
                    "dingtalk stream disconnected account={} retry_in_ms={delay}",
                    cfg.account_id
                );
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                }
            }
            Err(e) => {
                attempt = attempt.saturating_add(1);
                let delay = reconnect_delay(attempt);
                log::error!(
                    "dingtalk stream connection error account={}: {e:#} retry_in_ms={delay}",
                    cfg.account_id
                );
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                }
            }
        }
    }
}

enum StopReason {
    Cancelled,
    Disconnected,
}

async fn run_single_connection(
    cfg: &DingTalkStreamConfig,
    http: &HttpClient,
    gateway: std::sync::Arc<ChannelGateway>,
    cancel: CancellationToken,
    reconnect_attempt: &mut u32,
) -> Result<StopReason> {
    let (endpoint, ticket) = open_stream_ticket(http, cfg).await?;
    let ws_url = format!("{endpoint}?ticket={ticket}");
    log::info!("dingtalk stream connecting account={}", cfg.account_id);
    let (ws, _) = connect_async(&ws_url)
        .await
        .with_context(|| format!("dingtalk ws connect {ws_url}"))?;
    let (mut write, mut read) = ws.split();
    log::info!("dingtalk stream connected account={}", cfg.account_id);
    *reconnect_attempt = 0;
    let _connected = connection_state::ConnectionGuard::connect("dingtalk", &cfg.account_id);
    let dedup_ns = format!("dingtalk:{}", cfg.account_id);
    gateway.dedup.clear_namespace(&dedup_ns);
    log::info!(
        "dingtalk stream dedup cleared on connect account={}",
        cfg.account_id
    );

    let mut missed_pong = 0u32;
    let heartbeat = tokio::time::interval(Duration::from_millis(HEARTBEAT_INTERVAL_MS));
    tokio::pin!(heartbeat);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(StopReason::Cancelled),
            _ = heartbeat.tick() => {
                if missed_pong >= 2 {
                    log::warn!(
                        "dingtalk stream heartbeat timeout account={}",
                        cfg.account_id
                    );
                    return Ok(StopReason::Disconnected);
                }
                missed_pong += 1;
                if write.send(Message::Ping(vec![].into())).await.is_err() {
                    return Ok(StopReason::Disconnected);
                }
            }
            msg = read.next() => {
                let Some(msg) = msg else {
                    return Ok(StopReason::Disconnected);
                };
                missed_pong = 0;
                match msg.context("dingtalk ws read")? {
                    Message::Text(text) => {
                        let envelope: Value = serde_json::from_str(&text)
                            .context("dingtalk stream json")?;
                        if let Some(resp) = handle_envelope(&envelope, cfg, gateway.clone()).await {
                            let body = serde_json::to_string(&resp)?;
                            if let Err(e) = write.send(Message::Text(body)).await {
                                log::error!(
                                    "dingtalk stream ack failed account={}: {e:#}",
                                    cfg.account_id
                                );
                                return Ok(StopReason::Disconnected);
                            }
                        }
                        if should_disconnect(&envelope) {
                            log::info!(
                                "dingtalk stream server disconnect account={}",
                                cfg.account_id
                            );
                            return Ok(StopReason::Disconnected);
                        }
                    }
                    Message::Close(_) => return Ok(StopReason::Disconnected),
                    Message::Ping(payload) => {
                        if write.send(Message::Pong(payload)).await.is_err() {
                            return Ok(StopReason::Disconnected);
                        }
                    }
                    Message::Pong(_) => {}
                    _ => {}
                }
            }
        }
    }
}

async fn open_stream_ticket(http: &HttpClient, cfg: &DingTalkStreamConfig) -> Result<(String, String)> {
    let body = json!({
        "clientId": cfg.client_id,
        "clientSecret": cfg.client_secret,
        "subscriptions": [
            { "topic": "*", "type": "EVENT" },
            { "topic": BOT_MSG_TOPIC, "type": "CALLBACK" }
        ],
        "ua": "pointer-channels-rust/0.1.0"
    });
    let resp = http.post_json(OPEN_URL, &[], &body).await?;
    let endpoint = resp
        .get("endpoint")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("dingtalk stream missing endpoint"))?
        .to_string();
    let ticket = resp
        .get("ticket")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("dingtalk stream missing ticket"))?
        .to_string();
    Ok((endpoint, ticket))
}

fn should_disconnect(envelope: &Value) -> bool {
    envelope.get("type").and_then(|v| v.as_str()) == Some("SYSTEM")
        && envelope
            .get("headers")
            .and_then(|h| h.get("topic"))
            .and_then(|v| v.as_str())
            == Some("disconnect")
}

async fn handle_envelope(
    envelope: &Value,
    cfg: &DingTalkStreamConfig,
    gateway: std::sync::Arc<ChannelGateway>,
) -> Option<Value> {
    let msg_type = envelope.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let headers = envelope.get("headers").cloned().unwrap_or(Value::Null);
    let message_id = headers
        .get("messageId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let topic = headers.get("topic").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "SYSTEM" if topic == "ping" => {
            let opaque = envelope
                .get("data")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .and_then(|d| d.get("opaque").cloned())
                .unwrap_or(Value::Null);
            return Some(stream_ack(200, &message_id, json!({ "opaque": opaque })));
        }
        "CALLBACK" if topic == BOT_MSG_TOPIC => {
            if let Some(data_raw) = envelope.get("data").and_then(|v| v.as_str()) {
                if let Ok(event) = serde_json::from_str::<Value>(data_raw) {
                    if let Some(inbound) =
                        super::webhook::DingTalkWebhook.parse_inbound(&event, &cfg.account_id)
                    {
                        wait_process_inbound(gateway.clone(), inbound, &cfg.account_id).await;
                    }
                }
            }
            return Some(stream_ack(
                200,
                &message_id,
                json!({ "response": null }),
            ));
        }
        "EVENT" => {
            return Some(stream_ack(
                200,
                &message_id,
                json!({ "status": "SUCCESS", "message": "ok" }),
            ));
        }
        _ => {}
    }
    None
}

fn stream_ack(code: u16, message_id: &str, data: Value) -> Value {
    json!({
        "code": code,
        "message": if code == 200 { "OK" } else { "error" },
        "headers": {
            "messageId": message_id,
            "contentType": "application/json"
        },
        "data": serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
    })
}

async fn wait_process_inbound(
    gateway: std::sync::Arc<ChannelGateway>,
    inbound: crate::traits::InboundMessage,
    account_id: &str,
) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let result = gateway.process_inbound(inbound).await;
        let _ = tx.send(result);
    });
    match tokio::time::timeout(INBOUND_PROCESS_TIMEOUT, rx).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => {
            log::error!(
                "dingtalk stream process_inbound failed account={account_id}: {e:#}"
            );
        }
        Ok(Err(_)) => {
            log::warn!("dingtalk stream process_inbound channel closed account={account_id}");
        }
        Err(_) => {
            log::warn!(
                "dingtalk stream process_inbound still running after {}s account={account_id}",
                INBOUND_PROCESS_TIMEOUT.as_secs()
            );
        }
    }
}

fn reconnect_delay(attempt: u32) -> u64 {
    let exp = attempt.saturating_sub(1).min(8);
    (RECONNECT_BASE_MS * 2u64.pow(exp)).min(RECONNECT_MAX_MS)
}
