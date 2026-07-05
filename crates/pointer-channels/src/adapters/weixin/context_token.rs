//! Shared Weixin `context_token` cache and refresh (iLink getconfig + ret=-2 retry).

use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use parking_lot::Mutex;

use super::ilink_client::ILinkClient;

/// Proactively refresh via `getconfig` when the cached token is older than this.
pub const CONTEXT_TOKEN_MAX_AGE_MS: i64 = 45_000;

#[derive(Debug, Clone)]
struct Entry {
    token: String,
    cached_at_ms: i64,
}

static STORE: OnceLock<Mutex<HashMap<String, HashMap<String, Entry>>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, HashMap<String, Entry>>> {
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn set(account_id: &str, user_id: &str, token: &str) {
    if token.trim().is_empty() {
        return;
    }
    store()
        .lock()
        .entry(account_id.to_string())
        .or_default()
        .insert(
            user_id.to_string(),
            Entry {
                token: token.to_string(),
                cached_at_ms: now_ms(),
            },
        );
}

pub fn get(account_id: &str, user_id: &str) -> Option<String> {
    store()
        .lock()
        .get(account_id)?
        .get(user_id)
        .map(|e| e.token.clone())
}

fn token_age_ms(account_id: &str, user_id: &str) -> Option<i64> {
    let guard = store().lock();
    let entry = guard.get(account_id)?.get(user_id)?;
    Some(now_ms().saturating_sub(entry.cached_at_ms))
}

/// iLink returns `ret=-2 errmsg=unknown` when the cached token is stale (community-observed).
pub fn is_stale_session_send_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}");
    msg.contains("sendmessage ret=-2") && msg.contains("errmsg=unknown")
}

fn token_from_getconfig_response(resp: &serde_json::Value) -> Option<String> {
    resp.get("context_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(String::from)
}

async fn fetch_config_token(
    client: &ILinkClient,
    user_id: &str,
    current_token: Option<&str>,
) -> Result<Option<String>> {
    let resp = client.get_config(user_id, current_token).await?;
    Ok(token_from_getconfig_response(&resp))
}

/// Refresh `context_token` via iLink `getconfig`.
///
/// Tries with the current token first, then without one — some sessions only
/// recover when `getconfig` is called with no stale token attached.
pub async fn refresh_via_getconfig(
    client: &ILinkClient,
    user_id: &str,
    current_token: &str,
) -> Result<Option<String>> {
    if let Some(token) = fetch_config_token(client, user_id, Some(current_token)).await? {
        return Ok(Some(token));
    }
    fetch_config_token(client, user_id, None).await
}

async fn maybe_refresh_stale_token(
    client: &ILinkClient,
    account_id: &str,
    user_id: &str,
    token: &str,
) -> Result<String> {
    let age_ms = token_age_ms(account_id, user_id).unwrap_or(0);
    if age_ms < CONTEXT_TOKEN_MAX_AGE_MS {
        return Ok(token.to_string());
    }
    log::info!(
        "weixin context_token proactive refresh account={account_id} user={user_id} age_ms={age_ms}"
    );
    apply_refresh(client, account_id, user_id, token).await
}

async fn apply_refresh(
    client: &ILinkClient,
    account_id: &str,
    user_id: &str,
    token: &str,
) -> Result<String> {
    match refresh_via_getconfig(client, user_id, token).await {
        Ok(Some(new_token)) => {
            if new_token != token {
                log::info!(
                    "weixin context_token refreshed account={account_id} user={user_id}"
                );
            } else {
                log::info!(
                    "weixin context_token getconfig returned same token account={account_id} user={user_id}"
                );
            }
            set(account_id, user_id, &new_token);
            Ok(new_token)
        }
        Ok(None) => Ok(token.to_string()),
        Err(e) => {
            log::warn!(
                "weixin getconfig refresh failed account={account_id} user={user_id}: {e:#}"
            );
            Ok(token.to_string())
        }
    }
}

pub fn resolve_token(
    account_id: &str,
    user_id: &str,
    reply_context_token: Option<&str>,
) -> Result<String> {
    if let Some(from_reply) = reply_context_token.filter(|s| !s.trim().is_empty()) {
        if get(account_id, user_id).as_deref() != Some(from_reply) {
            set(account_id, user_id, from_reply);
        }
        return Ok(from_reply.to_string());
    }
    get(account_id, user_id).context(
        "weixin sendmessage requires context_token (user must message the bot first)",
    )
}

pub async fn resolve_token_for_send(
    client: &ILinkClient,
    account_id: &str,
    user_id: &str,
    reply_context_token: Option<&str>,
) -> Result<String> {
    let token = resolve_token(account_id, user_id, reply_context_token)?;
    if get(account_id, user_id).is_none() {
        set(account_id, user_id, &token);
    }
    maybe_refresh_stale_token(client, account_id, user_id, &token).await
}

async fn retry_after_stale_token<F, Fut>(
    client: &ILinkClient,
    account_id: &str,
    user_id: &str,
    token: &str,
    send: F,
) -> Result<()>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    match send(token.to_string()).await {
        Ok(()) => Ok(()),
        Err(e) if is_stale_session_send_error(&e) => {
            log::warn!(
                "weixin sendmessage ret=-2; refreshing context_token account={account_id} user={user_id}"
            );
            let Some(new_token) = refresh_via_getconfig(client, user_id, token).await? else {
                return Err(anyhow::anyhow!(
                    "weixin context_token stale (ret=-2); getconfig did not return a token"
                ));
            };
            set(account_id, user_id, &new_token);
            send(new_token).await
        }
        Err(e) => Err(e),
    }
}

pub async fn send_text_resilient(
    client: &ILinkClient,
    account_id: &str,
    user_id: &str,
    text: &str,
    reply_context_token: Option<&str>,
) -> Result<()> {
    let token =
        resolve_token_for_send(client, account_id, user_id, reply_context_token).await?;
    let text = text.to_string();
    retry_after_stale_token(client, account_id, user_id, &token, |token| {
        let client = client.clone();
        let user_id = user_id.to_string();
        let text = text.clone();
        async move { client.send_text(&user_id, &text, Some(&token)).await }
    })
    .await
}

pub async fn send_message_items_resilient(
    client: &ILinkClient,
    account_id: &str,
    user_id: &str,
    item_list: &mut [serde_json::Value],
    reply_context_token: Option<&str>,
) -> Result<()> {
    let token =
        resolve_token_for_send(client, account_id, user_id, reply_context_token).await?;
    let items = item_list.to_vec();
    retry_after_stale_token(client, account_id, user_id, &token, |token| {
        let client = client.clone();
        let user_id = user_id.to_string();
        let items = items.clone();
        async move {
            let mut batch = items;
            client
                .send_message_items(&user_id, &token, &mut batch)
                .await
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_session_error_detection() {
        let err = anyhow::anyhow!("iLink sendmessage ret=-2 errmsg=unknown");
        assert!(is_stale_session_send_error(&err));
        let other = anyhow::anyhow!("iLink sendmessage ret=-2 errmsg=bad param");
        assert!(!is_stale_session_send_error(&other));
    }

    #[test]
    fn store_set_get_and_age() {
        set("acct", "user@im.wechat", "tok-a");
        assert_eq!(get("acct", "user@im.wechat"), Some("tok-a".into()));
        assert!(token_age_ms("acct", "user@im.wechat").unwrap_or(-1) >= 0);
    }

    #[test]
    fn resolve_prefers_reply_context_over_cache() {
        set("acct2", "u@im.wechat", "cached");
        let got = resolve_token("acct2", "u@im.wechat", Some("from-reply")).unwrap();
        assert_eq!(got, "from-reply");
        assert_eq!(get("acct2", "u@im.wechat"), Some("from-reply".into()));
    }
}
