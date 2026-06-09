//! Post-turn background memory review (self-improvement, memory-only).

use super::store::{count_real_user_turns, memory_review_due};
use super::tool::{inject_memory_limits, plan_includes_memory};
use crate::chat_service::StreamTx;
use crate::memory::MemoryStore;
use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent, SystemPromptSections};
use crate::provider::OpenAIProvider;
use crate::tools::parse_tool_call_arguments;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const REVIEW_PROMPT: &str = include_str!("prompts/review_memory.md");
const REVIEW_MAX_ITERATIONS: u32 = 8;
const REVIEW_HISTORY_MSG_CAP: usize = 80;
const REVIEW_SNIPPET_CHARS: usize = 2500;

pub fn should_run_memory_review(
    settings: &ModelSettings,
    allowed_tool_names: &[String],
    history: &[ChatMessage],
) -> bool {
    if !settings.background_review_enabled {
        return false;
    }
    if settings.agent_mode != "single" || settings.lead_agent_id.trim() != "general" {
        return false;
    }
    if !settings.memory_enabled && !settings.user_profile_enabled {
        return false;
    }
    if !plan_includes_memory(allowed_tool_names) {
        return false;
    }
    let turns = count_real_user_turns(history);
    memory_review_due(turns, settings.memory_nudge_interval)
}

pub fn spawn_memory_background_review(
    state: Arc<crate::chat_service::AppState>,
    provider: OpenAIProvider,
    conversation_id: String,
    history_snapshot: Vec<ChatMessage>,
    settings: ModelSettings,
    stream: StreamTx,
) {
    tokio::spawn(async move {
        match run_memory_background_review(
            state.memory_store.clone(),
            state.tools.clone(),
            provider,
            &conversation_id,
            history_snapshot,
            &settings,
        )
        .await
        {
            Ok(summary) => {
                if let Some(msg) = summary {
                    let _ = stream.send(StreamEvent::UiToast {
                        conversation_id: conversation_id.clone(),
                        message: msg,
                        level: "success".into(),
                    });
                }
            }
            Err(e) => {
                log::warn!(
                    "memory background review failed conversation_id={}: {e:#}",
                    conversation_id
                );
            }
        }
    });
}

async fn run_memory_background_review(
    memory_store: Arc<MemoryStore>,
    tools: Arc<ToolRegistry>,
    provider: OpenAIProvider,
    conversation_id: &str,
    history_snapshot: Vec<ChatMessage>,
    settings: &ModelSettings,
) -> Result<Option<String>> {
    let allowed = vec!["memory".to_string()];
    let native_tools = tools.openai_tools(&allowed);
    if native_tools.is_empty() {
        return Err(anyhow!("memory tool not registered"));
    }

    let mut messages = trim_history_for_review(&history_snapshot);
    messages.push(review_user_message());

    let system = SystemPromptSections::all_cacheable(vec![]);
    let cancel = CancellationToken::new();
    let mut actions: Vec<String> = Vec::new();

    for iter in 0..REVIEW_MAX_ITERATIONS {
        let out = provider
            .chat_once(
                &messages,
                &system,
                native_tools.clone(),
                cancel.clone(),
                Some(settings.context_summary_max_tokens.min(2048)),
                Some(&format!("memory_review_{conversation_id}_{iter}")),
            )
            .await?;

        if out.tool_calls.is_empty() {
            log::info!(
                "memory background review: no tool calls conversation_id={} iter={}",
                conversation_id,
                iter
            );
            break;
        }

        let assistant_id = format!("mem_rev_a_{iter}");
        messages.push(ChatMessage {
            id: assistant_id.clone(),
            role: Role::Assistant,
            content: out.text.clone(),
            status: "done".into(),
            created_at: now_ms(),
            tool_calls: Some(out.tool_calls.clone()),
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
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
        });

        for tc in &out.tool_calls {
            if tc.name != "memory" {
                log::warn!(
                    "memory background review: unexpected tool {} conversation_id={}",
                    tc.name,
                    conversation_id
                );
                continue;
            }
            let mut args = parse_tool_call_arguments(&tc.arguments);
            inject_memory_limits(
                &mut args,
                settings.memory_char_limit,
                settings.user_char_limit,
            );
            let result = memory_store.dispatch_tool(&args).unwrap_or_else(|e| {
                json!({ "success": false, "error": e.to_string() }).to_string()
            });
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                if v.get("success").and_then(|b| b.as_bool()) == Some(true) {
                    let target = v
                        .get("target")
                        .and_then(|t| t.as_str())
                        .unwrap_or("memory");
                    let label = if target == "user" {
                        "User profile"
                    } else {
                        "Memory"
                    };
                    actions.push(format!("{label} updated"));
                }
            }
            messages.push(ChatMessage {
                id: format!("mem_rev_t_{iter}_{}", tc.id),
                role: Role::Tool,
                content: result,
                status: "done".into(),
                created_at: now_ms(),
                tool_calls: None,
                tool_call_id: Some(tc.id.clone()),
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
                image_slot_labels: None,
                images_base64: None,
                computer_round_screen_rel_path: None,
                ui_bindings: None,
                context_state: None,
                attachments: None,
            });
        }
    }

    if actions.is_empty() {
        return Ok(None);
    }
    let unique: Vec<String> = actions
        .into_iter()
        .fold(Vec::new(), |mut acc, a| {
            if !acc.contains(&a) {
                acc.push(a);
            }
            acc
        });
    Ok(Some(format!("已更新记忆：{}", unique.join(" · "))))
}

fn review_user_message() -> ChatMessage {
    ChatMessage {
        id: format!("mem_rev_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content: REVIEW_PROMPT.to_string(),
        status: "done".into(),
        created_at: now_ms(),
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
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
    }
}

fn trim_history_for_review(history: &[ChatMessage]) -> Vec<ChatMessage> {
    let start = history.len().saturating_sub(REVIEW_HISTORY_MSG_CAP);
    history[start..]
        .iter()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant | Role::Tool))
        .map(|m| {
            let mut c = m.clone();
            c.images_base64 = None;
            c.image_slot_labels = None;
            c.computer_round_screen_rel_path = None;
            c.content = truncate_chars(&c.content, REVIEW_SNIPPET_CHARS);
            c
        })
        .collect()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, Role};

    fn user_msg(content: &str) -> ChatMessage {
        ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            role: Role::User,
            content: content.into(),
            status: "done".into(),
            created_at: 0,
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
            image_slot_labels: None,
            images_base64: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
        }
    }

    #[test]
    fn should_review_when_interval_matches() {
        let mut settings = ModelSettings::default();
        settings.agent_mode = "single".into();
        settings.lead_agent_id = "general".into();
        settings.memory_nudge_interval = 2;
        settings.background_review_enabled = true;
        settings.memory_enabled = true;
        let history = vec![user_msg("a"), user_msg("b")];
        assert!(should_run_memory_review(
            &settings,
            &["memory".into()],
            &history
        ));
    }
}
