//! Post-turn background self-improvement review (memory / skill / combined).

use super::store::{count_real_user_turns, memory_review_due, skill_review_due};
use super::tool::{inject_memory_limits, plan_includes_memory};
use crate::chat_service::StreamTx;
use crate::memory::MemoryStore;
use crate::models::{ChatMessage, ModelSettings, Role, StreamEvent, SystemPromptSections};
use crate::provider::OpenAIProvider;
use crate::skills::SkillRegistry;
use crate::tools::parse_tool_call_arguments;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};
use serde_json::json;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const REVIEW_MEMORY_PROMPT: &str = include_str!("prompts/review_memory.md");
const REVIEW_SKILL_PROMPT: &str = include_str!("prompts/review_skill.md");
const REVIEW_COMBINED_PROMPT: &str = include_str!("prompts/review_combined.md");
const REVIEW_MAX_ITERATIONS: u32 = 8;
const REVIEW_HISTORY_MSG_CAP: usize = 80;
const REVIEW_SNIPPET_CHARS: usize = 2500;

const MEMORY_TOOL: &str = "memory";
const SKILL_READ: &str = "skill_read";
const SKILL_PATCH: &str = "skill_patch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewKind {
    MemoryOnly,
    SkillOnly,
    Combined,
}

fn review_base_eligible(settings: &ModelSettings) -> bool {
    settings.background_review_enabled
        && settings.agent_mode == "single"
        && settings.lead_agent_id.trim() == "general"
}

fn plan_includes_skill_tools(names: &[String]) -> bool {
    names.iter().any(|n| n == SKILL_READ || n == SKILL_PATCH)
}

pub fn memory_review_due_for(settings: &ModelSettings, allowed_tool_names: &[String], history: &[ChatMessage]) -> bool {
    if !review_base_eligible(settings) {
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

pub fn skill_review_due_for(
    settings: &ModelSettings,
    allowed_tool_names: &[String],
    cumulative_tool_iters: u32,
) -> bool {
    if !review_base_eligible(settings) {
        return false;
    }
    if !plan_includes_skill_tools(allowed_tool_names) {
        return false;
    }
    skill_review_due(cumulative_tool_iters, settings.skill_creation_nudge_interval)
}

pub fn resolve_review_kind(memory_due: bool, skill_due: bool) -> Option<ReviewKind> {
    match (memory_due, skill_due) {
        (true, true) => Some(ReviewKind::Combined),
        (true, false) => Some(ReviewKind::MemoryOnly),
        (false, true) => Some(ReviewKind::SkillOnly),
        (false, false) => None,
    }
}

/// Backward-compatible helper (memory-only trigger).
pub fn should_run_memory_review(
    settings: &ModelSettings,
    allowed_tool_names: &[String],
    history: &[ChatMessage],
) -> bool {
    memory_review_due_for(settings, allowed_tool_names, history)
}

pub fn spawn_background_review(
    state: Arc<crate::chat_service::AppState>,
    provider: OpenAIProvider,
    conversation_id: String,
    history_snapshot: Vec<ChatMessage>,
    settings: ModelSettings,
    enabled_skill_ids: Vec<String>,
    kind: ReviewKind,
    stream: StreamTx,
) {
    tokio::spawn(async move {
        match run_background_review(
            state.memory_store.clone(),
            state.skills.clone(),
            state.tools.clone(),
            provider,
            &conversation_id,
            history_snapshot,
            &settings,
            &enabled_skill_ids,
            kind,
        )
        .await
        {
            Ok(summary) => {
                if let Some(msg) = summary {
                    crate::stream_broadcast::publish_stream(
                        &stream,
                        StreamEvent::UiToast {
                            conversation_id: conversation_id.clone(),
                            message: msg,
                            level: "success".into(),
                        },
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "background review failed conversation_id={} kind={kind:?}: {e:#}",
                    conversation_id
                );
            }
        }
    });
}

/// Backward-compatible entry (memory-only).
pub fn spawn_memory_background_review(
    state: Arc<crate::chat_service::AppState>,
    provider: OpenAIProvider,
    conversation_id: String,
    history_snapshot: Vec<ChatMessage>,
    settings: ModelSettings,
    stream: StreamTx,
) {
    spawn_background_review(
        state,
        provider,
        conversation_id,
        history_snapshot,
        settings,
        Vec::new(),
        ReviewKind::MemoryOnly,
        stream,
    );
}

async fn run_background_review(
    memory_store: Arc<MemoryStore>,
    skills: Arc<SkillRegistry>,
    tools: Arc<ToolRegistry>,
    provider: OpenAIProvider,
    conversation_id: &str,
    history_snapshot: Vec<ChatMessage>,
    settings: &ModelSettings,
    enabled_skill_ids: &[String],
    kind: ReviewKind,
) -> Result<Option<String>> {
    let allowed = allowed_tools_for(kind);
    let native_tools = tools.openai_tools(&allowed);
    if native_tools.is_empty() {
        return Err(anyhow!("no review tools registered for kind={kind:?}"));
    }

    let mut messages = trim_history_for_review(&history_snapshot);
    messages.push(review_user_message(kind, enabled_skill_ids));

    let system = SystemPromptSections::all_cacheable(vec![]);
    let cancel = CancellationToken::new();
    let mut memory_actions: Vec<String> = Vec::new();
    let mut skill_actions: Vec<String> = Vec::new();

    for iter in 0..REVIEW_MAX_ITERATIONS {
        let out = provider
            .chat_once(
                &messages,
                &system,
                native_tools.clone(),
                cancel.clone(),
                Some(settings.context_summary_max_tokens.min(2048)),
                Some(&format!("bg_review_{conversation_id}_{kind:?}_{iter}")),
            )
            .await?;

        if out.tool_calls.is_empty() {
            log::info!(
                "background review: no tool calls conversation_id={} kind={kind:?} iter={iter}",
                conversation_id,
            );
            break;
        }

        let assistant_id = format!("bg_rev_a_{iter}");
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
            if !allowed.iter().any(|a| a == &tc.name) {
                log::warn!(
                    "background review: unexpected tool {} conversation_id={}",
                    tc.name,
                    conversation_id
                );
                continue;
            }
            let result = dispatch_review_tool(
                &memory_store,
                &skills,
                settings,
                &tc.name,
                &tc.arguments,
            )
            .unwrap_or_else(|e| json!({ "success": false, "error": e.to_string() }).to_string());

            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
                if v.get("success").and_then(|b| b.as_bool()) == Some(true) {
                    if tc.name == MEMORY_TOOL {
                        let target = v
                            .get("target")
                            .and_then(|t| t.as_str())
                            .unwrap_or("memory");
                        let label = if target == "user" {
                            "User profile"
                        } else {
                            "Memory"
                        };
                        memory_actions.push(format!("{label} updated"));
                    } else if tc.name == SKILL_PATCH {
                        let id = v
                            .get("skill_id")
                            .and_then(|t| t.as_str())
                            .unwrap_or("skill");
                        skill_actions.push(format!("Skill patched: {id}"));
                    }
                }
            }

            messages.push(ChatMessage {
                id: format!("bg_rev_t_{iter}_{}", tc.id),
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

    format_review_toast(&memory_actions, &skill_actions)
}

pub(crate) fn allowed_tools_for(kind: ReviewKind) -> Vec<String> {
    match kind {
        ReviewKind::MemoryOnly => vec![MEMORY_TOOL.into()],
        ReviewKind::SkillOnly => vec![SKILL_READ.into(), SKILL_PATCH.into()],
        ReviewKind::Combined => vec![
            MEMORY_TOOL.into(),
            SKILL_READ.into(),
            SKILL_PATCH.into(),
        ],
    }
}

pub(crate) fn dispatch_review_tool(
    memory_store: &MemoryStore,
    skills: &SkillRegistry,
    settings: &ModelSettings,
    name: &str,
    arguments: &str,
) -> Result<String> {
    let mut args = parse_tool_call_arguments(arguments);
    match name {
        MEMORY_TOOL => {
            inject_memory_limits(
                &mut args,
                settings.memory_char_limit,
                settings.user_char_limit,
            );
            memory_store.dispatch_tool(&args)
        }
        SKILL_READ => {
            let id = args
                .get("skill_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing skill_id"))?;
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .filter(|p| !p.trim().is_empty());
            skills.read(id, path)
        }
        SKILL_PATCH => {
            let id = args
                .get("skill_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing skill_id"))?;
            let body = args
                .get("body")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing body"))?;
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .filter(|p| !p.trim().is_empty());
            skills.patch(id, path, body)?;
            skills.reload_meta()?;
            Ok(json!({ "success": true, "skill_id": id }).to_string())
        }
        other => Err(anyhow!("unsupported review tool: {other}")),
    }
}

fn format_review_toast(memory_actions: &[String], skill_actions: &[String]) -> Result<Option<String>> {
    let memory = dedupe(memory_actions);
    let skills = dedupe(skill_actions);
    if memory.is_empty() && skills.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::new();
    if !memory.is_empty() {
        parts.push(format!("已更新记忆：{}", memory.join(" · ")));
    }
    if !skills.is_empty() {
        parts.push(format!("已更新技能：{}", skills.join(" · ")));
    }
    Ok(Some(parts.join(" · ")))
}

fn dedupe(items: &[String]) -> Vec<String> {
    items
        .iter()
        .fold(Vec::new(), |mut acc, a| {
            if !acc.contains(a) {
                acc.push(a.clone());
            }
            acc
        })
}

fn review_user_message(kind: ReviewKind, enabled_skill_ids: &[String]) -> ChatMessage {
    let mut content = match kind {
        ReviewKind::MemoryOnly => REVIEW_MEMORY_PROMPT.to_string(),
        ReviewKind::SkillOnly => REVIEW_SKILL_PROMPT.to_string(),
        ReviewKind::Combined => REVIEW_COMBINED_PROMPT.to_string(),
    };
    if !enabled_skill_ids.is_empty() {
        content.push_str("\n\nSkills enabled this session:\n");
        for id in enabled_skill_ids {
            content.push_str("- ");
            content.push_str(id);
            content.push('\n');
        }
    }
    ChatMessage {
        id: format!("bg_rev_{}", uuid::Uuid::new_v4().simple()),
        role: Role::User,
        content,
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
    fn memory_review_when_interval_matches() {
        let mut settings = ModelSettings::default();
        settings.agent_mode = "single".into();
        settings.lead_agent_id = "general".into();
        settings.memory_nudge_interval = 2;
        settings.background_review_enabled = true;
        settings.memory_enabled = true;
        let history = vec![user_msg("a"), user_msg("b")];
        assert!(memory_review_due_for(
            &settings,
            &["memory".into()],
            &history
        ));
    }

    #[test]
    fn skill_review_when_tool_iters_match() {
        let mut settings = ModelSettings::default();
        settings.agent_mode = "single".into();
        settings.lead_agent_id = "general".into();
        settings.skill_creation_nudge_interval = 10;
        settings.background_review_enabled = true;
        assert!(skill_review_due_for(
            &settings,
            &["skill_read".into()],
            10
        ));
    }

    #[test]
    fn combined_kind_when_both_due() {
        assert_eq!(
            resolve_review_kind(true, true),
            Some(ReviewKind::Combined)
        );
        assert_eq!(
            resolve_review_kind(true, false),
            Some(ReviewKind::MemoryOnly)
        );
        assert_eq!(resolve_review_kind(false, true), Some(ReviewKind::SkillOnly));
        assert_eq!(resolve_review_kind(false, false), None);
    }
}
