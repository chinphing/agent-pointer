use anyhow::Result;
use parking_lot::Mutex;
use pointer_core::agents::{AGENT_MODE_SINGLE, DEFAULT_LEAD_AGENT_ID};
use pointer_core::chat_service::{run_chat, AppState};
use pointer_core::conversation_store::im_session::ImSessionState;
use pointer_core::models::{ChatMessage, MediaAttachment, Role, StreamEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::media::resolve_inbound_attachments;
use crate::session::{conversation_id, inbound_user_message_id};
use crate::session_fork::{fork_im_desktop_session, resolve_active_desktop_id};
use crate::session_agent::{agent_switch_ack, detect_agent_switch, AgentSwitchAction};
use crate::session_reset::{self, ManualResetAction, MANUAL_RESET_ACK};
use crate::im_stream_outbound::ImStreamOutbound;
use crate::outbound_reply::{im_outbound_reply_source, split_reply_media};
use crate::traits::{ChannelPlugin, InboundMessage, OutboundContext};

fn broadcast_im_session_agent(
    desktop_conv_id: &str,
    base_conv_id: &str,
    session_state: &ImSessionState,
) {
    pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::ImSessionAgentChanged {
        conversation_id: desktop_conv_id.to_string(),
        base_conversation_id: base_conv_id.to_string(),
        lead_agent_id: session_state.lead_agent_id.clone(),
        agent_mode: session_state.agent_mode.clone(),
    });
}

fn sync_im_desktop_session_agent(
    store: &pointer_core::conversation_store::ConversationStore,
    desktop_conv_id: &str,
    session_state: &ImSessionState,
) {
    if let Err(e) = store.patch_session_agent(
        desktop_conv_id,
        &session_state.lead_agent_id,
        &session_state.agent_mode,
    ) {
        log::warn!(
            "channel patch session agent failed desktop={desktop_conv_id}: {e:#}"
        );
    }
}

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

fn lead_agent_override(session_state: &ImSessionState) -> Option<String> {
    if session_state.lead_agent_id.trim() == DEFAULT_LEAD_AGENT_ID {
        None
    } else {
        Some(session_state.lead_agent_id.clone())
    }
}

fn request_agent_mode(session_state: &ImSessionState) -> Option<String> {
    if session_state.agent_mode.trim() == AGENT_MODE_SINGLE {
        None
    } else {
        Some(session_state.agent_mode.clone())
    }
}

pub struct DispatchService {
    http: HttpClient,
    /// Serialize runs per IM conversation (OpenClaw-style one active turn per session).
    conv_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl DispatchService {
    pub fn new() -> Result<Self> {
        Ok(Self {
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
        let store = state.session_index.clone();
        let mut im_session = store.load_im_session(&conv_id)?;

        let mut session_forked = false;
        match session_reset::detect_manual_reset(&user_text) {
            Some(ManualResetAction::ResetOnly) => {
                if let Err(e) = fork_im_desktop_session(
                    &*store,
                    &conv_id,
                    &mut im_session,
                    msg.sender_name.as_deref(),
                ) {
                    log::warn!("channel session fork failed conv={conv_id}: {e:#}");
                }
                store.touch_im_interaction(&conv_id)?;
                plugin
                    .outbound
                    .send_text(outbound, MANUAL_RESET_ACK)
                    .await?;
                log::info!("channel session manual reset conv={conv_id}");
                return Ok(());
            }
            Some(ManualResetAction::ResetWithMessage(rest)) => {
                if let Err(e) = fork_im_desktop_session(
                    &*store,
                    &conv_id,
                    &mut im_session,
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
                    im_session.last_interaction_at_ms,
                    idle_minutes,
                    now_ms,
                ) {
                    if let Err(e) = fork_im_desktop_session(
                        &*store,
                        &conv_id,
                        &mut im_session,
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
            im_session
                .active_conversation_id
                .clone()
                .unwrap_or_else(|| resolve_active_desktop_id(&conv_id, &im_session))
        } else {
            resolve_active_desktop_id(&conv_id, &im_session)
        };

        match detect_agent_switch(&state.agents, &user_text) {
            Some(AgentSwitchAction::SwitchOnly(target)) => {
                im_session.agent_mode = target.agent_mode.clone();
                im_session.lead_agent_id = target
                    .lead_agent_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_LEAD_AGENT_ID.to_string());
                store.save_im_session(&conv_id, &im_session)?;
                store.touch_im_interaction(&conv_id)?;
                plugin
                    .outbound
                    .send_text(outbound.clone(), &agent_switch_ack(&target))
                    .await?;
                sync_im_desktop_session_agent(&*store, &desktop_conv_id, &im_session);
                broadcast_im_session_agent(&desktop_conv_id, &conv_id, &im_session);
                log::info!(
                    "channel session agent switch conv={conv_id} mode={} lead={:?}",
                    target.agent_mode,
                    target.lead_agent_id
                );
                return Ok(());
            }
            Some(AgentSwitchAction::SwitchWithMessage(target, rest)) => {
                im_session.agent_mode = target.agent_mode.clone();
                im_session.lead_agent_id = target
                    .lead_agent_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_LEAD_AGENT_ID.to_string());
                store.save_im_session(&conv_id, &im_session)?;
                user_text = rest;
                log::info!(
                    "channel session agent switch with message conv={conv_id} mode={} lead={:?}",
                    target.agent_mode,
                    target.lead_agent_id
                );
            }
            None => {}
        }

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

        if let Err(e) = store.upsert_message_no_refresh(&desktop_conv_id, &user_msg) {
            log::warn!(
                "channel upsert inbound user message failed desktop={desktop_conv_id}: {e:#}"
            );
        }

        let history = store.load_messages(&desktop_conv_id)?;

        if let Err(e) = store.ensure_im_title(
            &desktop_conv_id,
            msg.sender_name.as_deref(),
            Some(user_content.as_str()),
        ) {
            log::warn!(
                "channel ensure im title failed desktop={desktop_conv_id} base={conv_id}: {e:#}"
            );
        }

        pointer_core::stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: desktop_conv_id.clone(),
            message_id: user_msg.id.clone(),
            content: user_content.clone(),
            attachments: attachments_opt,
        });

        sync_im_desktop_session_agent(&*store, &desktop_conv_id, &im_session);
        broadcast_im_session_agent(&desktop_conv_id, &conv_id, &im_session);

        let workspace_root = store.workspace_root(&desktop_conv_id).unwrap_or_default();

        let channels_cfg = crate::config::load_channels_config().unwrap_or_default();
        let im_outbound_cfg = channels_cfg.meta.im_outbound.clone();

        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let mut reply_text = String::new();
        let mut stream_out = ImStreamOutbound::new(
            plugin,
            outbound.clone(),
            im_outbound_cfg,
            conv_id.clone(),
        );

        let run = run_chat(
            tx.clone(),
            state,
            desktop_conv_id.clone(),
            history,
            vec![],
            request_agent_mode(&im_session),
            lead_agent_override(&im_session),
            0,
            0,
            workspace_root,
        );

        // Keep collecting until StreamEvent::Done. Tool rounds emit an intermediate
        // MessageEnd with empty content; breaking early drops the final answer.
        let collect = async {
            while let Some(ev) = rx.recv().await {
                match &ev {
                    StreamEvent::Delta { text, .. } => reply_text.push_str(text),
                    StreamEvent::MessageEnd {
                        content,
                        raw_content,
                        ..
                    } => {
                        let next =
                            im_outbound_reply_source(raw_content.as_deref(), content.as_deref());
                        if !next.trim().is_empty() {
                            reply_text = next;
                        }
                        if let Err(e) = stream_out.on_event(&ev).await {
                            log::warn!(
                                "channel im stream outbound message_end failed conv={conv_id}: {e:#}"
                            );
                        }
                    }
                    StreamEvent::ToolCallStatus { .. } => {
                        if let Err(e) = stream_out.on_event(&ev).await {
                            log::warn!(
                                "channel im stream outbound tool_status failed conv={conv_id}: {e:#}"
                            );
                        }
                    }
                    StreamEvent::Error { message, .. } => {
                        if reply_text.trim().is_empty() && !message.is_empty() {
                            reply_text = message.clone();
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

        let (visible_text, media_refs) = split_reply_media(&reply_text);
        if visible_text.trim().is_empty() && media_refs.is_empty() {
            log::warn!("channel dispatch empty reply conv={conv_id}");
            return Err(anyhow::anyhow!("channel dispatch empty reply"));
        }

        store.touch_im_interaction(&conv_id)?;

        let sent = stream_out.finish(&reply_text).await?;
        if !sent && visible_text.trim().is_empty() && media_refs.is_empty() {
            log::warn!("channel dispatch empty outbound conv={conv_id}");
            return Err(anyhow::anyhow!("channel dispatch empty reply"));
        }

        Ok(())
    }
}
