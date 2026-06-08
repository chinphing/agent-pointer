use anyhow::Result;
use qrcode::QrCode;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::ilink_client::{parse_login_info, ILinkClient, WeixinCredentials};
use crate::credentials::save_encrypted_json;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrLoginSession {
    pub account_id: String,
    pub qrcode: String,
    pub qrcode_png_base64: String,
    pub status: String,
}

pub struct QrLoginState {
    sessions: Arc<RwLock<HashMap<String, QrLoginSession>>>,
}

impl QrLoginState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(&self, account_id: &str) -> Result<QrLoginSession> {
        let client = ILinkClient::new(
            account_id.to_string(),
            WeixinCredentials {
                bot_token: String::new(),
                account_id: account_id.to_string(),
                base_url: super::ilink_client::DEFAULT_BASE_URL.into(),
                user_id: String::new(),
            },
        );
        let resp = client.fetch_qrcode().await?;
        let qrcode_token = resp
            .get("qrcode")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing qrcode"))?
            .to_string();
        let qrcode_url = resp
            .get("qrcode_img_content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing qrcode_img_content"))?
            .to_string();
        let png_b64 = qrcode_png_base64(&qrcode_url)?;
        let session = QrLoginSession {
            account_id: account_id.to_string(),
            qrcode: qrcode_url,
            qrcode_png_base64: png_b64,
            status: "pending".into(),
        };
        self.sessions
            .write()
            .await
            .insert(account_id.to_string(), session.clone());

        let sessions = self.sessions.clone();
        let aid = account_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = poll_until_done(sessions, &aid, &qrcode_token).await {
                log::error!("weixin qr poll failed account={aid}: {e:#}");
            }
        });
        Ok(session)
    }

    pub async fn get(&self, account_id: &str) -> Option<QrLoginSession> {
        self.sessions.read().await.get(account_id).cloned()
    }
}

async fn poll_until_done(
    sessions: Arc<RwLock<HashMap<String, QrLoginSession>>>,
    account_id: &str,
    qrcode: &str,
) -> Result<()> {
    let client = ILinkClient::new(
        account_id.to_string(),
        WeixinCredentials {
            bot_token: String::new(),
            account_id: account_id.to_string(),
            base_url: super::ilink_client::DEFAULT_BASE_URL.into(),
            user_id: String::new(),
        },
    );
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let resp = client.poll_qrcode_status(qrcode).await?;
        let status = resp
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("wait");
        if status == "confirmed" {
            let creds = parse_login_info(&resp)?;
            save_encrypted_json("weixin", account_id, &creds)?;
            let mut guard = sessions.write().await;
            if let Some(s) = guard.get_mut(account_id) {
                s.status = "confirmed".into();
            }
            log::info!("weixin login confirmed account={account_id}");
            return Ok(());
        }
        if status == "scaned" {
            let mut guard = sessions.write().await;
            if let Some(s) = guard.get_mut(account_id) {
                s.status = "scanned".into();
            }
            continue;
        }
        if status == "expired" || status == "failed" {
            let mut guard = sessions.write().await;
            if let Some(s) = guard.get_mut(account_id) {
                s.status = status.into();
            }
            return Err(anyhow::anyhow!("qr login {status}"));
        }
    }
    Err(anyhow::anyhow!("qr login timeout"))
}

fn qrcode_png_base64(data: &str) -> Result<String> {
    use base64::Engine as _;
    use image::ImageEncoder;
    let code = QrCode::new(data.as_bytes())?;
    let image = code.render::<image::Luma<u8>>().build();
    let mut bytes: Vec<u8> = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    )?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
