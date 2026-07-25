//! After one provider stream round: assistant row, format retries, tool-budget exhaustion.
//! Shared by lead single-agent and sub-agent loops.

use crate::agents::agent_ui::agent_display_label;
use crate::agents::{AgentDef, AgentPlan, AgentRunResult};
use crate::models::{AgentTrace, ChatMessage, Role, StreamEvent, ToolCall};
use crate::tools::parse_tool_call_arguments;
use crate::tools::ToolRegistry;
use anyhow::{anyhow, Result};

use super::app_state::AppState;
use super::content_extract::{extract_user_visible_content, reply_attachments_from_assistant_raw};
use super::context::PostAssistantContext;
use super::emit::emit;
use super::sub_message::SubMessageLinkage;
use super::util::now_ms;
use super::StreamTx;

const CONSOLE_SEGMENT_MAX_CHARS: usize = 2000;

/// What the outer agent loop should do after persisting the assistant turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PostAssistantTurnAction {
    /// No tools and no format retry — end the run successfully.
    FinishRun,
    /// Valid tool batch — run tool pass.
    ExecuteTools,
}

fn assistant_tool_calls_with_risk(
    final_tool_calls: &[ToolCall],
    state: &AppState,
) -> Option<Vec<ToolCall>> {
    if final_tool_calls.is_empty() {
        return None;
    }
    Some(
        final_tool_calls
            .iter()
            .cloned()
            .map(|mut t| {
                let args_v = parse_tool_call_arguments(&t.arguments);
                t.risk_level = state
                    .tools
                    .tool_risk_level_for_invocation(&t.name, &args_v)
                    .or(Some("low".into()));
                let display = state.tools.format_display(&t.name, &args_v);
                t.display_label = Some(display.label);
                t.display_summary = if display.summary.is_empty() {
                    None
                } else {
                    Some(display.summary)
                };
                t
            })
            .collect(),
    )
}

fn compact_console_segment(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "(empty)".to_string();
    }
    let chars = t.chars().count();
    if chars <= CONSOLE_SEGMENT_MAX_CHARS {
        return t.to_string();
    }
    let head: String = t.chars().take(CONSOLE_SEGMENT_MAX_CHARS).collect();
    format!(
        "{head}…(+{} chars)",
        chars.saturating_sub(CONSOLE_SEGMENT_MAX_CHARS)
    )
}

pub(super) fn log_reasoning_and_output_segments(
    scope: &str,
    message_id: &str,
    reasoning: Option<&str>,
    thoughts: Option<&str>,
    output: Option<&str>,
    tool_raw_output: Option<&str>,
) {
    let reasoning_text = reasoning
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| thoughts.map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("");
    let output_text = output
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let tool_text = tool_raw_output
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    if reasoning_text.is_empty() && output_text.is_empty() && tool_text.is_empty() {
        return;
    }
    if !crate::logging::internal_runtime_log_enabled() {
        return;
    }
    log::debug!(
        "assistant_segments scope={} message_id={}\n[推理|reasoning]\n{}\n[输出|output]\n{}\n[工具原始输出|tool_raw_output]\n{}",
        scope,
        message_id,
        compact_console_segment(reasoning_text),
        compact_console_segment(output_text),
        compact_console_segment(tool_text)
    );
}

/// Final assistant bubble after a lone successful [`ToolEntry::final_reply`] tool.
pub(super) fn build_final_reply_delivery_message(
    assistant_id: &str,
    tool_output: &str,
    agent_plan: &AgentPlan,
    agent_instance_id: Option<String>,
    _state: &AppState,
) -> ChatMessage {
    let content = crate::media::strip_outbound_media_markers(tool_output);
    let attachments = reply_attachments_from_assistant_raw(tool_output);
    ChatMessage {
        id: assistant_id.to_string(),
        role: Role::Assistant,
        content,
        status: "completed".into(),
        created_at: now_ms(),
        tool_calls: None,
        tool_call_id: None,
        error_message: None,
        reasoning: None,
        thoughts: None,
        headline: None,
        raw_content: Some(tool_output.to_string()),
        tool_raw_output: None,
        agent_id: Some(agent_plan.lead_agent_id.clone()),
        agent_instance_id,
        agent_name: Some(agent_plan.lead_agent_name.clone()),
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    }
}

pub(super) fn build_lead_assistant_message_after_stream(
    assistant_id: &str,
    raw_content_buf: &str,
    reasoning_buf: String,
    reasoning_in_messages: bool,
    final_tool_calls: &[ToolCall],
    xml_thoughts: Option<String>,
    agent_plan: &AgentPlan,
    agent_instance_id: Option<String>,
    agent_trace: &[AgentTrace],
    state: &AppState,
) -> ChatMessage {
    ChatMessage {
        id: assistant_id.to_string(),
        role: Role::Assistant,
        content: extract_user_visible_content(raw_content_buf),
        status: if final_tool_calls.is_empty() {
            "completed".into()
        } else {
            "streaming".into()
        },
        created_at: now_ms(),
        tool_calls: assistant_tool_calls_with_risk(final_tool_calls, state),
        tool_call_id: None,
        error_message: None,
        reasoning: if reasoning_in_messages && !reasoning_buf.is_empty() {
            Some(reasoning_buf)
        } else {
            None
        },
        thoughts: xml_thoughts,
        headline: None,
        raw_content: if raw_content_buf.is_empty() {
            None
        } else {
            Some(raw_content_buf.to_string())
        },
        tool_raw_output: None,
        agent_id: Some(agent_plan.lead_agent_id.clone()),
        agent_instance_id,
        agent_name: Some(agent_plan.lead_agent_name.clone()),
        agent_trace: if agent_trace.is_empty() {
            None
        } else {
            Some(agent_trace.to_vec())
        },
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: reply_attachments_from_assistant_raw(raw_content_buf),
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    }
}

pub(super) fn build_sub_assistant_message_after_stream(
    round_message_id: &str,
    round_content: String,
    round_reasoning: String,
    reasoning_in_messages: bool,
    final_tool_calls: &[ToolCall],
    round_thoughts: Option<String>,
    def: &AgentDef,
    agent_instance_id: Option<String>,
    state: &AppState,
) -> ChatMessage {
    ChatMessage {
        id: round_message_id.to_string(),
        role: Role::Assistant,
        content: round_content,
        status: if final_tool_calls.is_empty() {
            "completed".into()
        } else {
            "streaming".into()
        },
        created_at: now_ms(),
        tool_calls: assistant_tool_calls_with_risk(final_tool_calls, state),
        tool_call_id: None,
        error_message: None,
        reasoning: if reasoning_in_messages && !round_reasoning.is_empty() {
            Some(round_reasoning)
        } else {
            None
        },
        thoughts: round_thoughts,
        headline: None,
        raw_content: None,
        tool_raw_output: None,
        agent_id: Some(def.id.clone()),
        agent_instance_id,
        agent_name: Some(agent_display_label(def)),
        agent_trace: None,
        image_slot_labels: None,
        images_base64: None,
        computer_round_screen_rel_path: None,
        ui_bindings: None,
        context_state: None,
        attachments: None,
        anchor_message_id: None,
        trace_id: None,
        task_id: None,
        spawn_depth: None,
    }
}

pub(super) fn commit_sub_assistant_turn(
    stream: &StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    assistant_msg: ChatMessage,
    linkage: &SubMessageLinkage,
) {
    log_reasoning_and_output_segments(
        "sub",
        &assistant_msg.id,
        assistant_msg.reasoning.as_deref(),
        assistant_msg.thoughts.as_deref(),
        Some(assistant_msg.content.as_str()),
        assistant_msg.tool_raw_output.as_deref(),
    );
    history.push(assistant_msg.clone());
    super::sub_message::persist_sub_message(conversation_id, linkage, &assistant_msg);
    emit(
        stream,
        StreamEvent::MessageEnd {
            message_id: linkage.anchor_message_id.clone(),
            content: Some(assistant_msg.content.clone()),
            raw_content: assistant_msg.raw_content.clone(),
            tool_raw_output: assistant_msg.tool_raw_output.clone(),
            thoughts: assistant_msg.thoughts.clone(),
            headline: assistant_msg.headline.clone(),
            trace_id: Some(linkage.trace_id.clone()),
            scoped_message_id: Some(assistant_msg.id.clone()),
            attachments: assistant_msg.attachments.clone(),
        },
    );
}

pub(super) fn commit_lead_assistant_turn(
    stream: &StreamTx,
    conversation_id: &str,
    history: &mut Vec<ChatMessage>,
    assistant_id: &str,
    assistant_msg: &ChatMessage,
) {
    log_reasoning_and_output_segments(
        "lead",
        assistant_id,
        assistant_msg.reasoning.as_deref(),
        assistant_msg.thoughts.as_deref(),
        Some(assistant_msg.content.as_str()),
        assistant_msg.tool_raw_output.as_deref(),
    );
    if let Some(existing) = history.iter_mut().find(|m| m.id == assistant_id) {
        *existing = assistant_msg.clone();
    } else {
        history.push(assistant_msg.clone());
    }
    super::conversation_persist::upsert_message(conversation_id, assistant_msg);
    emit(
        stream,
        StreamEvent::MessageEnd {
            message_id: assistant_id.to_string(),
            content: Some(assistant_msg.content.clone()),
            raw_content: assistant_msg.raw_content.clone(),
            tool_raw_output: assistant_msg.tool_raw_output.clone(),
            thoughts: assistant_msg.thoughts.clone(),
            headline: assistant_msg.headline.clone(),
            trace_id: None,
            scoped_message_id: None,
            attachments: assistant_msg.attachments.clone(),
        },
    );
}

/// Empty tool batch: optional JSON format retry, or successful stop.
pub(super) async fn decide_when_no_tool_calls(
    ctx: &mut PostAssistantContext<'_>,
) -> Result<PostAssistantTurnAction> {
    if let Some(consumed) = ctx.budget.consumed_single.as_mut() {
        ctx.budget.tool_budget.sync_out(consumed);
    }
    Ok(PostAssistantTurnAction::FinishRun)
}

/// Non-empty tool batch: envelope validation or proceed to execution.
pub(super) async fn decide_when_tool_calls_present(
    _tools: &ToolRegistry,
    _final_tool_calls: &[ToolCall],
    _log_prefix: &str,
) -> Result<PostAssistantTurnAction> {
    Ok(PostAssistantTurnAction::ExecuteTools)
}

/// After optional `tool_budget.sync_out`, if the budget is exhausted: emit, compress, cancel, and fail.
pub(super) async fn bail_on_tool_budget_exhausted(
    ctx: &mut PostAssistantContext<'_>,
) -> Result<()> {
    if !ctx.budget.tool_budget.is_exhausted() {
        return Ok(());
    }
    let scope = ctx.budget.budget_scope;
    emit(
        ctx.session.stream,
        StreamEvent::ToolRoundsExhausted {
            conversation_id: ctx.session.conversation_id.to_string(),
            max_rounds: ctx.budget.max_cap,
            message: scope.user_hint.clone(),
            will_retry_after_compress: ctx.llm.settings.context_compression_enabled,
        },
    );
    let _ = crate::context_compression::maybe_compress_after_tool_round_limit(
        ctx.transcript.history,
        ctx.llm.settings,
        ctx.llm.provider,
        ctx.session.conversation_id,
        ctx.session.stream,
        ctx.session.cancel.clone(),
        scope.compress_for_session,
        crate::context_compression::CompressionUiContext::main(scope.compression_scope.clone()),
        None,
    )
    .await;
    if let Some(consumed) = ctx.budget.consumed_single.as_mut() {
        ctx.budget.tool_budget.sync_out(consumed);
    }
    ctx.session
        .state
        .computer_state
        .mark_cancelled(ctx.session.conversation_id);
    Err(anyhow!(scope.error_message.clone()))
}

pub(super) fn sub_agent_run_result(
    task_id: &str,
    def: &AgentDef,
    content: String,
    reasoning_in_messages: bool,
    reasoning: String,
) -> AgentRunResult {
    AgentRunResult {
        task_id: task_id.to_string(),
        agent_id: def.id.clone(),
        agent_name: agent_display_label(def),
        content,
        reasoning: if reasoning_in_messages && !reasoning.is_empty() {
            Some(reasoning)
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AgentPlan;

    fn sample_tool_call(name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: "tc_display".into(),
            name: name.into(),
            arguments: arguments.into(),
            status: "pending".into(),
            result: None,
            error: None,
            duration_ms: None,
            risk_level: None,
            display_label: None,
            display_summary: None,
        }
    }

    #[test]
    fn assistant_tool_calls_fill_display_fields_for_file_read() {
        let state = AppState::new();
        let tool_calls = vec![sample_tool_call(
            "file_read",
            r#"{"path":"src/components/App.vue"}"#,
        )];
        let plan = AgentPlan {
            mode: "single".into(),
            lead_agent_id: "coder".into(),
            lead_agent_name: "Coder".into(),
            system_prompts: vec![],
            active_def: crate::agents::AgentDef {
                id: "coder".into(),
                name: "Coder".into(),
                description: String::new(),
                role: "worker".into(),
                profile: crate::agents::AgentProfile::Coder,
                default_skill_ids: vec![],
                skills_policy: crate::agents::SkillsPolicy::Disabled,
                access_policy: crate::agents::AccessPolicy::default(),
                builtin: true,
                enabled: true,
                tool_names: vec![],
                source: None,
                resource_files: vec![],
                allow_agents: vec![],
                config: std::collections::HashMap::new(),
                ui: crate::agents::AgentUiConfig::default(),
            },
            active_system_prompt: String::new(),
            resolved_skill_ids: vec![],
            resolved_skill_prompts: vec![],
            allowed_tool_names: vec![],
            allow_agents: vec![],
        };
        let msg = build_lead_assistant_message_after_stream(
            "asst_1",
            "",
            String::new(),
            false,
            &tool_calls,
            None,
            &plan,
            None,
            &[],
            &state,
        );
        let tcs = msg.tool_calls.expect("tool_calls");
        assert_eq!(tcs[0].display_label.as_deref(), Some("读取文件"));
        assert_eq!(tcs[0].display_summary.as_deref(), Some("App.vue"));
    }

    #[test]
    fn assistant_tool_calls_fill_display_fields_for_terminal() {
        let state = AppState::new();
        let tool_calls = vec![sample_tool_call("terminal", r#"{"command":"npm test"}"#)];
        let plan = AgentPlan {
            mode: "single".into(),
            lead_agent_id: "coder".into(),
            lead_agent_name: "Coder".into(),
            system_prompts: vec![],
            active_def: crate::agents::AgentDef {
                id: "coder".into(),
                name: "Coder".into(),
                description: String::new(),
                role: "worker".into(),
                profile: crate::agents::AgentProfile::Coder,
                default_skill_ids: vec![],
                skills_policy: crate::agents::SkillsPolicy::Disabled,
                access_policy: crate::agents::AccessPolicy::default(),
                builtin: true,
                enabled: true,
                tool_names: vec![],
                source: None,
                resource_files: vec![],
                allow_agents: vec![],
                config: std::collections::HashMap::new(),
                ui: crate::agents::AgentUiConfig::default(),
            },
            active_system_prompt: String::new(),
            resolved_skill_ids: vec![],
            resolved_skill_prompts: vec![],
            allowed_tool_names: vec![],
            allow_agents: vec![],
        };
        let msg = build_lead_assistant_message_after_stream(
            "asst_2",
            "",
            String::new(),
            false,
            &tool_calls,
            None,
            &plan,
            None,
            &[],
            &state,
        );
        let tcs = msg.tool_calls.expect("tool_calls");
        assert_eq!(tcs[0].display_label.as_deref(), Some("终端命令"));
        assert_eq!(tcs[0].display_summary.as_deref(), Some("npm test"));
    }
}
