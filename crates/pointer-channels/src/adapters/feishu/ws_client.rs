use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::connection_state;
use crate::adapters::feishu::parse::parse_feishu_event;
use crate::gateway::ChannelGateway;
use crate::http_client::HttpClient;
use crate::traits::InboundMessage;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/pbbp2.rs"));
}

use proto::{Frame, Header};

/// 与 larksuite/oapi-sdk-go `GenEndpointUri` 一致：`FeishuBaseUrl + "/callback/ws/endpoint"`
const ENDPOINT_PATH: &str = "https://open.feishu.cn/callback/ws/endpoint";
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(120);
const INBOUND_PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const RECONNECT_BASE_MS: u64 = 1_000;
const RECONNECT_MAX_MS: u64 = 30_000;

pub struct FeishuWsConfig {
    pub account_id: String,
    pub app_id: String,
    pub app_secret: String,
}

pub async fn run_feishu_ws_loop(
    cfg: FeishuWsConfig,
    gateway: std::sync::Arc<ChannelGateway>,
    cancel: CancellationToken,
) -> Result<()> {
    let http = HttpClient::new()?;
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            log::info!("feishu ws loop cancelled account={}", cfg.account_id);
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
                    "feishu ws disconnected account={} retry_in_ms={delay}",
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
                    "feishu ws connection error account={}: {e:#} retry_in_ms={delay}",
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

#[derive(Debug, Deserialize)]
struct EndpointResponse {
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "ClientConfig")]
    client_config: Option<ClientConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct ClientConfig {
    #[serde(rename = "PingInterval")]
    ping_interval: Option<i32>,
}

async fn run_single_connection(
    cfg: &FeishuWsConfig,
    http: &HttpClient,
    gateway: std::sync::Arc<ChannelGateway>,
    cancel: CancellationToken,
    reconnect_attempt: &mut u32,
) -> Result<StopReason> {
    let endpoint = open_endpoint(http, cfg).await?;
    let url = endpoint
        .url
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("feishu ws missing endpoint url"))?;
    let parsed = Url::parse(url).context("feishu ws url")?;
    let service_id: i32 = parsed
        .query_pairs()
        .find(|(k, _)| k == "service_id")
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("feishu ws missing service_id"))?;

    log::info!("feishu ws connecting account={}", cfg.account_id);
    let (ws, _) = connect_async(url).await.context("feishu ws connect")?;
    let (mut write, mut read) = ws.split();
    log::info!("feishu ws connected account={}", cfg.account_id);
    *reconnect_attempt = 0;
    let _connected = connection_state::ConnectionGuard::connect("feishu", &cfg.account_id);
    let dedup_ns = format!("feishu:{}", cfg.account_id);
    gateway.dedup.clear_namespace(&dedup_ns);
    log::info!("feishu ws dedup cleared on connect account={}", cfg.account_id);

    let ping_secs = endpoint
        .client_config
        .as_ref()
        .and_then(|c| c.ping_interval)
        .unwrap_or(30)
        .max(5) as u64;
    let mut ping_interval = tokio::time::interval(Duration::from_secs(ping_secs));
    let mut last_pong = Instant::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(StopReason::Cancelled),
            _ = ping_interval.tick() => {
                let frame = build_ping_frame(service_id);
                if let Err(e) = write.send(Message::Binary(frame.encode_to_vec())).await {
                    log::error!("feishu ws ping failed account={}: {e:#}", cfg.account_id);
                    return Ok(StopReason::Disconnected);
                }
            }
            msg = read.next() => {
                let Some(msg) = msg else {
                    return Ok(StopReason::Disconnected);
                };
                match msg.context("feishu ws read")? {
                    Message::Binary(data) => {
                        let frame = Frame::decode(data.as_slice()).context("feishu ws frame")?;
                        if frame.method == 0 {
                            if header_value(&frame.headers, "type") == Some("pong".into()) {
                                last_pong = Instant::now();
                            }
                            continue;
                        }
                        if frame.method != 1 {
                            continue;
                        }
                        if header_value(&frame.headers, "type").as_deref() != Some("event") {
                            continue;
                        }
                        let payload = frame.payload.clone().unwrap_or_default();
                        if let Ok(root) = serde_json::from_slice::<Value>(&payload) {
                            if let Some(inbound) =
                                parse_feishu_event(&root, &cfg.account_id)
                            {
                                wait_process_inbound(gateway.clone(), inbound, &cfg.account_id)
                                    .await;
                            }
                        }
                        let response = json!({ "code": 200, "headers": {}, "data": [] });
                        let mut resp_frame = frame;
                        resp_frame.payload = Some(serde_json::to_vec(&response)?);
                        write
                            .send(Message::Binary(resp_frame.encode_to_vec()))
                            .await?;
                    }
                    Message::Ping(data) => {
                        let _ = write.send(Message::Pong(data)).await;
                        last_pong = Instant::now();
                    }
                    Message::Close(_) => return Ok(StopReason::Disconnected),
                    _ => {}
                }
                if last_pong.elapsed() > HEARTBEAT_TIMEOUT {
                    log::warn!("feishu ws heartbeat timeout account={}", cfg.account_id);
                    return Ok(StopReason::Disconnected);
                }
            }
        }
    }
}

async fn open_endpoint(http: &HttpClient, cfg: &FeishuWsConfig) -> Result<EndpointResponse> {
    let body = json!({
        "AppID": cfg.app_id,
        "AppSecret": cfg.app_secret
    });
    let resp = http
        .post_json(ENDPOINT_PATH, &[("locale", "zh")], &body)
        .await?;
    if resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) != 0 {
        return Err(anyhow::anyhow!(
            "feishu ws endpoint failed: {}",
            resp.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown")
        ));
    }
    let data = resp
        .get("data")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("feishu ws endpoint missing data"))?;
    Ok(serde_json::from_value(data)?)
}

fn build_ping_frame(service_id: i32) -> Frame {
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: 0,
        headers: vec![Header {
            key: "type".to_string(),
            value: "ping".to_string(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

fn header_value(headers: &[Header], key: &str) -> Option<String> {
    headers
        .iter()
        .find(|h| h.key == key)
        .map(|h| h.value.clone())
}

/// 尽量在 ack 前完成处理；超时后后台继续，避免重连时 dedup 误杀重投消息。
async fn wait_process_inbound(
    gateway: std::sync::Arc<ChannelGateway>,
    inbound: InboundMessage,
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
                "feishu ws process_inbound failed account={account_id}: {e:#}"
            );
        }
        Ok(Err(_)) => {
            log::warn!("feishu ws process_inbound channel closed account={account_id}");
        }
        Err(_) => {
            log::warn!(
                "feishu ws process_inbound still running after {}s account={account_id}",
                INBOUND_PROCESS_TIMEOUT.as_secs()
            );
        }
    }
}

fn reconnect_delay(attempt: u32) -> u64 {
    let exp = attempt.saturating_sub(1).min(8);
    (RECONNECT_BASE_MS * 2u64.pow(exp)).min(RECONNECT_MAX_MS)
}
