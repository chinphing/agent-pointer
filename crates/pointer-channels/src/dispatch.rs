use anyhow::Result;
use parking_lot::Mutex;
use pointer_core::chat_service::{run_chat, AppState};
use pointer_core::models::{ChatMessage, MediaAttachment, Role, StreamEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::inbound::{ChannelHistoryStore, SessionArchiveReason};
use crate::media::resolve_inbound_attachments;
use crate::session::conversation_id;
use crate::session_reset::{self, ManualResetAction, MANUAL_RESET_ACK};
use crate::outbound_reply::split_reply_media;
use crate::outbound_resolve::resolve_outbound_media_with_policy;
use crate::session_context;
use crate::traits::{ChannelPlugin, InboundMessage, OutboundContext};

fn channel_message(role: Role, content: String, attachments: Option<Vec<MediaAttachment>>) -> ChatMessage {
    ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role,
        content,
        status: "done".into(),
        created_at: chrono::Utc::now().timestamp_millis(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: None,
        agent_instance_id: None,
        agent_name: None,
        agent_trace: None,
        images_base64: None,
        image_slot_labels: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments,
    }
}

pub struct DispatchService {
    pub history: ChannelHistoryStore,
    http: HttpClient,
    /// Serialize runs per IM conversation (OpenClaw-style one active turn per session).
    conv_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl DispatchService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            history: ChannelHistoryStore::new(),
            http: HttpClient::new()?,
            conv_locks: Mutex::new(HashMap::new()),
        })
    }

    fn conv_lock(&self, conversation_id: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self.conv_locks.lock();
        locks
            .entry(conversation_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    pub async fn handle_inbound(
        &self,
        state: Arc<AppState>,
        plugin: &ChannelPlugin,
        account: &ChannelAccountConfig,
        idle_minutes: u32,
        msg: InboundMessage,
    ) -> Result<()> {
        let conv_id = conversation_id(&msg);
        let conv_mutex = self.conv_lock(&conv_id);
        let _conv_guard = conv_mutex.lock().await;
        log::info!(
            "channel inbound channel={} account={} conv={} sender={}",
            msg.channel,
            msg.account_id,
            conv_id,
            msg.sender_id
        );

        if msg.is_group {
            if account.group_policy == "disabled" {
                log::info!("channel group policy disabled; drop");
                return Ok(());
            }
            if account.group_policy == "allowlist"
                && !account.group_allow_from.is_empty()
                && !account
                    .group_allow_from
                    .iter()
                    .any(|g| g == &msg.conversation_key)
            {
                log::info!("channel group not in allowlist; drop");
                return Ok(());
            }
            if account.require_mention && !msg.mentioned_bot {
                log::info!("channel group mention required; drop");
                return Ok(());
            }
        }

        let outbound = OutboundContext {
            channel: msg.channel.clone(),
            account_id: msg.account_id.clone(),
            conversation_key: msg.conversation_key.clone(),
            recipient_id: msg.sender_id.clone(),
            reply_context: msg.reply_context.clone(),
        };

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut user_text = msg.text.clone();

        match session_reset::detect_manual_reset(&user_text) {
            Some(ManualResetAction::ResetOnly) => {
                self.history
                    .reset_session(&conv_id, SessionArchiveReason::Manual)?;
                self.history.touch_meta(&conv_id)?;
                plugin
                    .outbound
                    .send_text(outbound, MANUAL_RESET_ACK)
                    .await?;
                log::info!("channel session manual reset conv={conv_id}");
                return Ok(());
            }
            Some(ManualResetAction::ResetWithMessage(rest)) => {
                self.history
                    .reset_session(&conv_id, SessionArchiveReason::Manual)?;
                user_text = rest;
                log::info!("channel session manual reset with follow-up conv={conv_id}");
            }
            None => {
                let meta = self.history.load_meta(&conv_id)?;
                if session_reset::should_idle_reset(
                    meta.last_interaction_at,
                    idle_minutes,
                    now_ms,
                ) {
                    self.history
                        .reset_session(&conv_id, SessionArchiveReason::Idle)?;
                    log::info!(
                        "channel session idle reset conv={conv_id} idle_minutes={idle_minutes}"
                    );
                }
            }
        }

        let mut history = self.history.load(&conv_id)?;
        let media_attachments = resolve_inbound_attachments(&self.http, account, &msg).await?;
        let mut user_content = user_text;
        if !media_attachments.is_empty() {
            let failed = msg.attachments.len().saturating_sub(media_attachments.len());
            if failed > 0 {
                log::warn!(
                    "channel inbound {failed}/{} attachment(s) failed to download",
                    msg.attachments.len()
                );
                if user_content.trim().is_empty() {
                    user_content = format!(
                        "[{} attachment(s) could not be downloaded]",
                        failed
                    );
                }
            }
        }
        let attachments_opt = if media_attachments.is_empty() {
            None
        } else {
            Some(media_attachments)
        };
        history.push(channel_message(Role::User, user_content.clone(), attachments_opt));

        if let Err(e) = state.session_index.ensure_im_title(
            &conv_id,
            msg.sender_name.as_deref(),
            Some(user_content.as_str()),
        ) {
            log::warn!("channel ensure im title failed conv={conv_id}: {e:#}");
        }

        session_context::register(&conv_id, outbound.clone());
        pointer_core::channel_outbound::register_im_session(&conv_id);

        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let mut reply_text = String::new();
        let mut assistant_message_id = String::new();

        let run = run_chat(
            tx.clone(),
            state,
            conv_id.clone(),
            history.clone(),
            vec![],
            None,
            0,
            0,
            String::new(),
        );

        // Keep collecting until StreamEvent::Done. Tool rounds emit an intermediate
        // MessageEnd with empty content; breaking early drops the final answer.
        let collect = async {
            while let Some(ev) = rx.recv().await {
                match ev {
                    StreamEvent::MessageStart { message_id, .. } => {
                        assistant_message_id = message_id;
                    }
                    StreamEvent::Delta { text, .. } => reply_text.push_str(&text),
                    StreamEvent::MessageEnd { content, .. } => {
                        if let Some(c) = content {
                            if !c.trim().is_empty() {
                                reply_text = c;
                            }
                        }
                    }
                    StreamEvent::Error { message, .. } => {
                        if reply_text.trim().is_empty() && !message.is_empty() {
                            reply_text = message;
                        }
                        break;
                    }
                    StreamEvent::Done { .. } => break,
                    _ => {}
                }
            }
        };

        let (run_res, ()) = tokio::join!(run, collect);
        pointer_core::channel_outbound::unregister_im_session(&conv_id);
        session_context::unregister(&conv_id);
        if let Err(e) = run_res {
            if e.to_string().contains("已停止生成") {
                log::info!("channel dispatch run cancelled conv={conv_id}");
                return Ok(());
            }
            return Err(e);
        }

        let media_roots = crate::config::load_channels_config()
            .map(|c| c.meta.media_local_roots)
            .unwrap_or_default();

        let (visible_text, media_refs) = split_reply_media(&reply_text);
        if visible_text.trim().is_empty() && media_refs.is_empty() {
            log::warn!("channel dispatch empty reply conv={conv_id}");
            return Err(anyhow::anyhow!("channel dispatch empty reply"));
        }

        history.push(channel_message(Role::Assistant, reply_text.clone(), None));
        self.history.save(&conv_id, &history)?;
        self.history.touch_meta(&conv_id)?;

        if !visible_text.trim().is_empty() {
            plugin
                .outbound
                .send_text(outbound.clone(), &visible_text)
                .await?;
        }

        for raw_path in media_refs.iter() {
            match resolve_outbound_media_with_policy(raw_path, &media_roots) {
                Ok(resolved) => {
                    if let Err(e) = plugin
                        .outbound
                        .send_media(outbound.clone(), None, resolved.media)
                        .await
                    {
                        log::error!(
                            "channel outbound media failed conv={conv_id} path={raw_path}: {e:#}"
                        );
                    }
                }
                Err(e) => {
                    log::error!(
                        "channel outbound media resolve failed conv={conv_id} path={raw_path}: {e:#}"
                    );
                }
            }
        }

        log::info!("channel outbound ok conv={conv_id}");
        Ok(())
    }
}

impl Default for DispatchService {
    fn default() -> Self {
        Self::new().expect("dispatch http client")
    }
}
