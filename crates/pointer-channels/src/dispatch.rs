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
use crate::session::{conversation_id, inbound_user_message_id};
use crate::session_fork::{fork_im_desktop_session, resolve_active_desktop_id};
use crate::session_agent::{agent_switch_ack, detect_agent_switch, AgentSwitchAction};
use crate::session_reset::{self, ManualResetAction, MANUAL_RESET_ACK};
use crate::outbound_reply::split_reply_media;
use crate::outbound_resolve::resolve_outbound_media_with_policy;
use crate::traits::{ChannelPlugin, InboundMessage, OutboundContext};

fn channel_message(role: Role, content: String, attachments: Option<Vec<MediaAttachment>>) -> ChatMessage {
    channel_message_with_id(uuid::Uuid::new_v4().to_string(), role, content, attachments)
}

fn channel_message_with_id(
    id: String,
    role: Role,
    content: String,
    attachments: Option<Vec<MediaAttachment>>,
) -> ChatMessage {
    ChatMessage {
        id,
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
        let mut session_meta = self.history.load_meta(&conv_id)?;

        let mut session_forked = false;
        match session_reset::detect_manual_reset(&user_text) {
            Some(ManualResetAction::ResetOnly) => {
                self.history
                    .reset_session(&conv_id, SessionArchiveReason::Manual)?;
                if let Err(e) = fork_im_desktop_session(
                    &self.history,
                    &state.session_index,
                    &conv_id,
                    &mut session_meta,
                    msg.sender_name.as_deref(),
                ) {
                    log::warn!("channel session fork failed conv={conv_id}: {e:#}");
                }
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
                if let Err(e) = fork_im_desktop_session(
                    &self.history,
                    &state.session_index,
                    &conv_id,
                    &mut session_meta,
                    msg.sender_name.as_deref(),
                ) {
                    log::warn!("channel session fork failed conv={conv_id}: {e:#}");
                } else {
                    session_forked = true;
                }
                user_text = rest;
                log::info!("channel session manual reset with follow-up conv={conv_id}");
            }
            None => {
                if session_reset::should_idle_reset(
                    session_meta.last_interaction_at,
                    idle_minutes,
                    now_ms,
                ) {
                    self.history
                        .reset_session(&conv_id, SessionArchiveReason::Idle)?;
                    if let Err(e) = fork_im_desktop_session(
                        &self.history,
                        &state.session_index,
                        &conv_id,
                        &mut session_meta,
                        msg.sender_name.as_deref(),
                    ) {
                        log::warn!("channel session idle fork failed conv={conv_id}: {e:#}");
                    } else {
                        session_forked = true;
                    }
                    log::info!(
                        "channel session idle reset conv={conv_id} idle_minutes={idle_minutes}"
                    );
                }
            }
        }

        let desktop_conv_id = if session_forked {
            session_meta
                .active_conversation_id
                .clone()
                .unwrap_or_else(|| resolve_active_desktop_id(&conv_id, &session_meta))
        } else {
            resolve_active_desktop_id(&conv_id, &session_meta)
        };

        match detect_agent_switch(&state.agents, &user_text) {
            Some(AgentSwitchAction::SwitchOnly(target)) => {
                session_meta.agent_mode = Some(target.agent_mode.clone());
                session_meta.lead_agent_id = target.lead_agent_id.clone();
                self.history.save_meta(&conv_id, &session_meta)?;
                self.history.touch_meta(&conv_id)?;
                plugin
                    .outbound
                    .send_text(outbound.clone(), &agent_switch_ack(&target))
                    .await?;
                log::info!(
                    "channel session agent switch conv={conv_id} mode={} lead={:?}",
                    target.agent_mode,
                    target.lead_agent_id
                );
                return Ok(());
            }
            Some(AgentSwitchAction::SwitchWithMessage(target, rest)) => {
                session_meta.agent_mode = Some(target.agent_mode.clone());
                session_meta.lead_agent_id = target.lead_agent_id.clone();
                self.history.save_meta(&conv_id, &session_meta)?;
                user_text = rest;
                log::info!(
                    "channel session agent switch with message conv={conv_id} mode={} lead={:?}",
                    target.agent_mode,
                    target.lead_agent_id
                );
            }
            None => {}
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
        let user_msg_id = inbound_user_message_id(&msg.channel, &msg.message_id);
        let user_msg = channel_message_with_id(
            user_msg_id,
            Role::User,
            user_content.clone(),
            attachments_opt.clone(),
        );
        history.push(user_msg.clone());

        if let Err(e) = state.session_index.ensure_im_title(
            &desktop_conv_id,
            msg.sender_name.as_deref(),
            Some(user_content.as_str()),
        ) {
            log::warn!(
                "channel ensure im title failed desktop={desktop_conv_id} base={conv_id}: {e:#}"
            );
        }

        // Mirror inbound user row only — do not HistoryReplaced with channel_histories
        // (that store lacks tool traces and can wipe the desktop transcript in memory).
        pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: desktop_conv_id.clone(),
            message_id: user_msg.id.clone(),
            content: user_content.clone(),
            attachments: attachments_opt,
        });

        let request_agent_mode = session_meta.agent_mode.clone();
        let lead_agent_override = session_meta.lead_agent_id.clone();

        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let mut reply_text = String::new();
        let mut assistant_message_id = String::new();

        let run = run_chat(
            tx.clone(),
            state,
            desktop_conv_id.clone(),
            history.clone(),
            vec![],
            request_agent_mode,
            lead_agent_override,
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
