use anyhow::Result;
use pointer_core::chat_service::{run_chat, AppState};
use pointer_core::models::{ChatMessage, MediaAttachment, Role, StreamEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::config::ChannelAccountConfig;
use crate::http_client::HttpClient;
use crate::inbound::ChannelHistoryStore;
use crate::media::resolve_inbound_attachments;
use crate::session::conversation_id;
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
}

impl DispatchService {
    pub fn new() -> Result<Self> {
        Ok(Self {
            history: ChannelHistoryStore::new(),
            http: HttpClient::new()?,
        })
    }

    pub async fn handle_inbound(
        &self,
        state: Arc<AppState>,
        plugin: &ChannelPlugin,
        account: &ChannelAccountConfig,
        msg: InboundMessage,
    ) -> Result<()> {
        let conv_id = conversation_id(&msg);
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

        let mut history = self.history.load(&conv_id)?;
        let media_attachments = resolve_inbound_attachments(&self.http, account, &msg).await?;
        let mut user_content = msg.text.clone();
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
        history.push(channel_message(Role::User, user_content, attachments_opt));

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
        run_res?;

        if reply_text.trim().is_empty() {
            log::warn!("channel dispatch empty reply conv={conv_id}");
            return Err(anyhow::anyhow!("channel dispatch empty reply"));
        }

        history.push(channel_message(Role::Assistant, reply_text.clone(), None));
        self.history.save(&conv_id, &history)?;

        let outbound = OutboundContext {
            channel: msg.channel.clone(),
            account_id: msg.account_id.clone(),
            conversation_key: msg.conversation_key.clone(),
            recipient_id: msg.sender_id.clone(),
            reply_context: msg.reply_context.clone(),
        };
        plugin.outbound.send_text(outbound, &reply_text).await?;
        log::info!("channel outbound ok conv={conv_id}");
        Ok(())
    }
}

impl Default for DispatchService {
    fn default() -> Self {
        Self::new().expect("dispatch http client")
    }
}
