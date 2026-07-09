use anyhow::Result;

use crate::config::ChannelsConfig;

use super::RegistrationSession;

fn ensure_account<'a>(
    cfg: &'a mut ChannelsConfig,
    channel: &str,
    account_id: &str,
) -> Result<&'a mut crate::config::ChannelAccountConfig> {
    let map = match channel {
        "feishu" => &mut cfg.feishu,
        "dingtalk" => &mut cfg.dingtalk,
        "wecom" => &mut cfg.wecom,
        other => anyhow::bail!("channel {other} does not support QR registration"),
    };
    Ok(map.entry(account_id.to_string()).or_default())
}

/// Merge QR registration credentials into channels config. Returns true when updated.
pub fn apply_registration_session_to_config(
    cfg: &mut ChannelsConfig,
    channel: &str,
    account_id: &str,
    session: &RegistrationSession,
) -> Result<bool> {
    let account = ensure_account(cfg, channel, account_id)?;
    match channel {
        "feishu" => {
            let (Some(app_id), Some(app_secret)) = (&session.app_id, &session.app_secret) else {
                return Ok(false);
            };
            account.app_id = app_id.clone();
            account.app_secret = app_secret.clone();
        }
        "dingtalk" => {
            let (Some(client_id), Some(client_secret)) =
                (&session.client_id, &session.client_secret)
            else {
                return Ok(false);
            };
            account.client_id = client_id.clone();
            account.client_secret = client_secret.clone();
        }
        "wecom" => {
            let (Some(bot_id), Some(secret)) = (&session.bot_id, &session.secret) else {
                return Ok(false);
            };
            account.bot_id = bot_id.clone();
            account.secret = secret.clone();
        }
        other => anyhow::bail!("channel {other} does not support QR registration"),
    }
    account.enabled = true;
    account.connection_mode = "websocket".into();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_wecom_registration_sets_websocket_credentials() {
        let mut cfg = ChannelsConfig::default();
        let session = RegistrationSession {
            channel: "wecom".into(),
            account_id: "default".into(),
            qr_url: String::new(),
            qrcode_png_base64: String::new(),
            status: "success".into(),
            app_id: None,
            app_secret: None,
            client_id: None,
            client_secret: None,
            bot_id: Some("bot-1".into()),
            secret: Some("sec-1".into()),
            error_message: None,
        };
        assert!(apply_registration_session_to_config(&mut cfg, "wecom", "default", &session).unwrap());
        let account = cfg.wecom.get("default").unwrap();
        assert_eq!(account.bot_id, "bot-1");
        assert_eq!(account.secret, "sec-1");
        assert!(account.enabled);
        assert_eq!(account.connection_mode, "websocket");
    }
}
