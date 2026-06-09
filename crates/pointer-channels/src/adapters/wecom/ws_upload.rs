use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde_json::json;
use std::time::Duration;
use tokio::sync::oneshot;

use super::ws_client::generate_req_id;
use super::ws_pending;
use super::ws_state::WeComWsSession;
use crate::crypto::md5_hex;

const CHUNK_SIZE: usize = 512 * 1024;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);

mod cmd {
    pub const INIT: &str = "aibot_upload_media_init";
    pub const CHUNK: &str = "aibot_upload_media_chunk";
    pub const FINISH: &str = "aibot_upload_media_finish";
}

fn check_frame(frame: &serde_json::Value, step: &str) -> Result<()> {
    let errcode = frame.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
    if errcode != 0 {
        let errmsg = frame
            .get("errmsg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        anyhow::bail!("wecom ws {step} failed: {errmsg} (code={errcode})");
    }
    Ok(())
}

async fn wait_frame(rx: oneshot::Receiver<serde_json::Value>, step: &str) -> Result<serde_json::Value> {
    let frame = tokio::time::timeout(UPLOAD_TIMEOUT, rx)
        .await
        .with_context(|| format!("wecom ws {step} timeout"))?
        .with_context(|| format!("wecom ws {step} channel closed"))?;
    check_frame(&frame, step)?;
    Ok(frame)
}

pub async fn upload_media(
    session: &WeComWsSession,
    bytes: &[u8],
    media_type: &str,
    filename: &str,
) -> Result<String> {
    if bytes.is_empty() {
        anyhow::bail!("wecom upload empty file");
    }
    let total_chunks = bytes.len().div_ceil(CHUNK_SIZE);
    if total_chunks > 100 {
        anyhow::bail!("wecom upload exceeds 100 chunks");
    }

    let init_req_id = generate_req_id("upload_init");
    let init_rx = ws_pending::register(init_req_id.clone());
    session.send_frame(json!({
        "cmd": cmd::INIT,
        "headers": { "req_id": init_req_id },
        "body": {
            "type": media_type,
            "filename": filename,
            "total_size": bytes.len(),
            "total_chunks": total_chunks,
            "md5": md5_hex(bytes)
        }
    }))?;
    let init_resp = wait_frame(init_rx, "upload_init").await?;
    let upload_id = init_resp
        .get("body")
        .and_then(|b| b.get("upload_id"))
        .and_then(|v| v.as_str())
        .context("wecom upload_id missing")?;

    for (index, chunk) in bytes.chunks(CHUNK_SIZE).enumerate() {
        let chunk_req_id = generate_req_id(&format!("upload_chunk_{index}"));
        let chunk_rx = ws_pending::register(chunk_req_id.clone());
        session.send_frame(json!({
            "cmd": cmd::CHUNK,
            "headers": { "req_id": chunk_req_id },
            "body": {
                "upload_id": upload_id,
                "chunk_index": index,
                "base64_data": B64.encode(chunk)
            }
        }))?;
        wait_frame(chunk_rx, &format!("upload_chunk_{index}")).await?;
    }

    let finish_req_id = generate_req_id("upload_finish");
    let finish_rx = ws_pending::register(finish_req_id.clone());
    session.send_frame(json!({
        "cmd": cmd::FINISH,
        "headers": { "req_id": finish_req_id },
        "body": { "upload_id": upload_id }
    }))?;
    let finish_resp = wait_frame(finish_rx, "upload_finish").await?;
    let media_id = finish_resp
        .get("body")
        .and_then(|b| b.get("media_id"))
        .and_then(|v| v.as_str())
        .context("wecom media_id missing")?;
    log::info!("wecom ws upload ok type={media_type} media_id={media_id}");
    Ok(media_id.to_string())
}
