use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use crate::connection_state;
use crate::adapters::wecom::parse::parse_ws_inbound;

pub const DEFAULT_WS_URL: &str = "wss://openws.work.weixin.qq.com";
const HEARTBEAT_INTERVAL_MS: u64 = 30_000;
const RECONNECT_BASE_MS: u64 = 1_000;
const RECONNECT_MAX_MS: u64 = 30_000;
const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const MAX_AUTH_FAILURE_ATTEMPTS: u32 = 5;
const SCENE_POINTER: i64 = 1;

mod cmd {
    pub const SUBSCRIBE: &str = "aibot_subscribe";
    pub const HEARTBEAT: &str = "ping";
    pub const RESPONSE: &str = "aibot_respond_msg";
    pub const SEND_MSG: &str = "aibot_send_msg";
    pub const CALLBACK: &str = "aibot_msg_callback";
    pub const EVENT_CALLBACK: &str = "aibot_event_callback";
}

pub fn generate_req_id(prefix: &str) -> String {
    let ts = chrono::Utc::now().timestamp_millis();
    let rand = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}_{ts}_{rand}")
}

pub struct WeComWsConfig {
    pub account_id: String,
    pub bot_id: String,
    pub secret: String,
    pub ws_url: String,
}

pub async fn run_wecom_ws_loop(
    cfg: WeComWsConfig,
    mut outbound_rx: mpsc::UnboundedReceiver<super::ws_state::WsOutboundCmd>,
    inbound_tx: mpsc::UnboundedSender<(Value, String)>,
    mut inbound_rx: mpsc::UnboundedReceiver<(Value, String)>,
    gateway: std::sync::Arc<crate::gateway::ChannelGateway>,
    cancel: CancellationToken,
) -> Result<()> {
    let mut reconnect_attempts = 0u32;
    let mut auth_failure_attempts = 0u32;

    loop {
        if cancel.is_cancelled() {
            log::info!("wecom ws loop cancelled account={}", cfg.account_id);
            break;
        }

        match run_single_connection(
            &cfg,
            &mut outbound_rx,
            &inbound_tx,
            &mut inbound_rx,
            &gateway,
            cancel.clone(),
        )
        .await
        {
            Ok(StopReason::Cancelled) => break,
            Ok(StopReason::Kicked) => {
                log::error!(
                    "wecom ws kicked by server (duplicate connection?) account={}",
                    cfg.account_id
                );
                break;
            }
            Ok(StopReason::Disconnected) => {
                auth_failure_attempts = 0;
                if reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                    log::error!(
                        "wecom ws reconnect exhausted account={}",
                        cfg.account_id
                    );
                    break;
                }
                reconnect_attempts += 1;
                let delay = (RECONNECT_BASE_MS * 2u64.pow(reconnect_attempts.saturating_sub(1)))
                    .min(RECONNECT_MAX_MS);
                log::warn!(
                    "wecom ws reconnecting account={} attempt={}/{} delay_ms={}",
                    cfg.account_id,
                    reconnect_attempts,
                    MAX_RECONNECT_ATTEMPTS,
                    delay
                );
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                }
            }
            Ok(StopReason::AuthFailed) => {
                reconnect_attempts = 0;
                auth_failure_attempts += 1;
                if auth_failure_attempts >= MAX_AUTH_FAILURE_ATTEMPTS {
                    log::error!(
                        "wecom ws auth failed after {MAX_AUTH_FAILURE_ATTEMPTS} attempts account={}",
                        cfg.account_id
                    );
                    break;
                }
                let delay = (RECONNECT_BASE_MS * 2u64.pow(auth_failure_attempts.saturating_sub(1)))
                    .min(RECONNECT_MAX_MS);
                log::warn!(
                    "wecom ws auth retry account={} attempt={}/{}",
                    cfg.account_id,
                    auth_failure_attempts,
                    MAX_AUTH_FAILURE_ATTEMPTS
                );
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                }
            }
            Err(e) => {
                log::error!(
                    "wecom ws connection error account={}: {e:#}",
                    cfg.account_id
                );
                reconnect_attempts += 1;
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(RECONNECT_BASE_MS)) => {}
                }
            }
        }
    }
    Ok(())
}

enum StopReason {
    Cancelled,
    Disconnected,
    AuthFailed,
    Kicked,
}

async fn run_single_connection(
    cfg: &WeComWsConfig,
    outbound_rx: &mut mpsc::UnboundedReceiver<super::ws_state::WsOutboundCmd>,
    inbound_tx: &mpsc::UnboundedSender<(Value, String)>,
    inbound_rx: &mut mpsc::UnboundedReceiver<(Value, String)>,
    gateway: &std::sync::Arc<crate::gateway::ChannelGateway>,
    cancel: CancellationToken,
) -> Result<StopReason> {
    let ws_url = if cfg.ws_url.trim().is_empty() {
        DEFAULT_WS_URL
    } else {
        cfg.ws_url.trim()
    };
    log::info!("wecom ws connecting account={} url={ws_url}", cfg.account_id);

    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("websocket connect failed")?;
    let (mut write, mut read) = ws_stream.split();

    let auth_req_id = generate_req_id(cmd::SUBSCRIBE);
    let auth_frame = json!({
        "cmd": cmd::SUBSCRIBE,
        "headers": { "req_id": auth_req_id },
        "body": {
            "bot_id": cfg.bot_id,
            "secret": cfg.secret,
            "scene": SCENE_POINTER,
            "plug_version": format!("pointer-channels/{}", env!("CARGO_PKG_VERSION"))
        }
    });
    write
        .send(Message::Text(auth_frame.to_string().into()))
        .await
        .context("send auth failed")?;

    let mut authenticated = false;
    let mut connected_guard: Option<connection_state::ConnectionGuard> = None;
    let mut missed_pong = 0u32;
    let mut pending_outbound: VecDeque<super::ws_state::WsOutboundCmd> = VecDeque::new();
    let heartbeat = tokio::time::interval(Duration::from_millis(HEARTBEAT_INTERVAL_MS));
    tokio::pin!(heartbeat);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(StopReason::Cancelled),
            inbound = inbound_rx.recv() => {
                if let Some((body, req_id)) = inbound {
                    if let Some(msg) = parse_ws_inbound(&body, &cfg.account_id, &req_id) {
                        let gw = gateway.clone();
                        let account_id = cfg.account_id.clone();
                        tokio::spawn(async move {
                            if let Err(e) = gw.process_inbound(msg).await {
                                log::error!(
                                    "wecom ws process_inbound failed account={account_id}: {e:#}"
                                );
                            }
                        });
                    }
                }
            }
            maybe_out = outbound_rx.recv() => {
                let Some(cmd) = maybe_out else { return Ok(StopReason::Disconnected); };
                if !authenticated {
                    log::info!(
                        "wecom ws outbound buffered until auth account={} pending={}",
                        cfg.account_id,
                        pending_outbound.len() + 1
                    );
                    pending_outbound.push_back(cmd);
                    continue;
                }
                if let Err(e) = send_outbound(&mut write, cmd).await {
                    log::error!(
                        "wecom ws outbound failed account={}: {e:#}",
                        cfg.account_id
                    );
                    return Ok(StopReason::Disconnected);
                }
            }
            msg = read.next() => {
                let Some(msg) = msg else {
                    return Ok(StopReason::Disconnected);
                };
                match msg.context("ws read")? {
                    Message::Text(text) => {
                        let frame: Value = serde_json::from_str(&text)
                            .context("parse ws frame")?;
                        let was_authenticated = authenticated;
                        if let Some(reason) = handle_inbound_frame(
                            &frame,
                            &auth_req_id,
                            inbound_tx,
                            &mut authenticated,
                            &mut missed_pong,
                        ).await? {
                            return Ok(reason);
                        }
                        if !was_authenticated && authenticated {
                            if connected_guard.is_none() {
                                connected_guard = Some(connection_state::ConnectionGuard::connect(
                                    "wecom",
                                    &cfg.account_id,
                                ));
                            }
                            let ns = format!("wecom:{}", cfg.account_id);
                            gateway.dedup.clear_namespace(&ns);
                            log::info!(
                                "wecom ws authenticated account={} flushing {} buffered outbound(s)",
                                cfg.account_id,
                                pending_outbound.len()
                            );
                            while let Some(cmd) = pending_outbound.pop_front() {
                                if let Err(e) = send_outbound(&mut write, cmd).await {
                                    log::error!(
                                        "wecom ws flush outbound failed account={}: {e:#}",
                                        cfg.account_id
                                    );
                                    return Ok(StopReason::Disconnected);
                                }
                            }
                        }
                    }
                    Message::Ping(data) => {
                        write.send(Message::Pong(data)).await.ok();
                    }
                    Message::Close(_) => return Ok(StopReason::Disconnected),
                    _ => {}
                }
            }
            _ = heartbeat.tick(), if authenticated => {
                if missed_pong >= 2 {
                    log::warn!("wecom ws heartbeat timeout account={}", cfg.account_id);
                    return Ok(StopReason::Disconnected);
                }
                missed_pong += 1;
                let ping = json!({
                    "cmd": cmd::HEARTBEAT,
                    "headers": { "req_id": generate_req_id(cmd::HEARTBEAT) }
                });
                if write.send(Message::Text(ping.to_string().into())).await.is_err() {
                    return Ok(StopReason::Disconnected);
                }
            }
        }
    }
}

async fn handle_inbound_frame(
    frame: &Value,
    auth_req_id: &str,
    inbound_tx: &mpsc::UnboundedSender<(Value, String)>,
    authenticated: &mut bool,
    missed_pong: &mut u32,
) -> Result<Option<StopReason>> {
    if super::ws_pending::dispatch_response(frame) {
        return Ok(None);
    }

    let cmd = frame.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    let req_id = frame
        .get("headers")
        .and_then(|h| h.get("req_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if cmd == cmd::CALLBACK {
        if let Some(body) = frame.get("body") {
            let _ = inbound_tx.send((body.clone(), req_id));
        }
        return Ok(None);
    }

    if cmd == cmd::EVENT_CALLBACK {
        if frame
            .get("body")
            .and_then(|b| b.get("event"))
            .and_then(|e| e.get("eventtype"))
            .and_then(|v| v.as_str())
            == Some("disconnected_event")
        {
            return Ok(Some(StopReason::Kicked));
        }
        return Ok(None);
    }

    if cmd.is_empty() {
        if req_id.starts_with(cmd::SUBSCRIBE) || req_id == auth_req_id {
            let errcode = frame.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
            if errcode != 0 {
                let errmsg = frame
                    .get("errmsg")
                    .and_then(|v| v.as_str())
                    .unwrap_or("auth failed");
                log::error!("wecom ws auth failed: {errmsg} (code={errcode})");
                return Ok(Some(StopReason::AuthFailed));
            }
            *authenticated = true;
            log::info!("wecom ws authenticated");
            return Ok(None);
        }
        if req_id.starts_with(cmd::HEARTBEAT) {
            let errcode = frame.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
            if errcode == 0 {
                *missed_pong = 0;
            }
            return Ok(None);
        }
    }

    Ok(None)
}

fn media_msg_body(msgtype: &str, media_id: &str) -> Value {
    let mut body = json!({ "msgtype": msgtype });
    if let Some(obj) = body.as_object_mut() {
        obj.insert(
            msgtype.to_string(),
            json!({ "media_id": media_id }),
        );
    }
    body
}

async fn send_outbound(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    cmd: super::ws_state::WsOutboundCmd,
) -> Result<()> {
    let frame = match cmd {
        super::ws_state::WsOutboundCmd::StreamReply {
            req_id,
            stream_id,
            content,
            finish,
        } => json!({
            "cmd": cmd::RESPONSE,
            "headers": { "req_id": req_id },
            "body": {
                "msgtype": "stream",
                "stream": {
                    "id": stream_id,
                    "finish": finish,
                    "content": content
                }
            }
        }),
        super::ws_state::WsOutboundCmd::SendMarkdown { chat_id, content } => {
            let req_id = generate_req_id(cmd::SEND_MSG);
            json!({
                "cmd": cmd::SEND_MSG,
                "headers": { "req_id": req_id },
                "body": {
                    "chatid": chat_id,
                    "msgtype": "markdown",
                    "markdown": { "content": content }
                }
            })
        }
        super::ws_state::WsOutboundCmd::SendFrame(frame) => frame,
        super::ws_state::WsOutboundCmd::RespondMedia {
            req_id,
            msgtype,
            media_id,
        } => json!({
            "cmd": cmd::RESPONSE,
            "headers": { "req_id": req_id },
            "body": media_msg_body(&msgtype, &media_id)
        }),
        super::ws_state::WsOutboundCmd::SendMedia {
            chat_id,
            msgtype,
            media_id,
        } => {
            let req_id = generate_req_id(cmd::SEND_MSG);
            let mut body = media_msg_body(&msgtype, &media_id);
            if let Some(obj) = body.as_object_mut() {
                obj.insert("chatid".into(), json!(chat_id));
            }
            json!({
                "cmd": cmd::SEND_MSG,
                "headers": { "req_id": req_id },
                "body": body
            })
        }
    };
    write
        .send(Message::Text(frame.to_string().into()))
        .await
        .map_err(|e| anyhow!("ws send failed: {e}"))
}
