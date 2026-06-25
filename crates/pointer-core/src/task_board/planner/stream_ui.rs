//! Stream planner tool rounds to the chat UI (tool cards on lead or scoped sub-agent rows).

use crate::chat_service::{
    emit, patch_assistant_tool_call_display, patch_assistant_tool_call_outcome,
    tool_display_stream_fields, AppState, StreamTx, trace_id_opt,
};
use crate::models::{ChatMessage, StreamEvent, ToolCall};
use crate::tools::parse_tool_call_arguments;
use serde_json::Value;

use super::tool_pass::PlannerToolOutcome;

pub const PLANNER_PHASE_THOUGHTS: &str = "正在规划任务…";

pub struct PlannerUiTarget<'a> {
    pub stream: &'a StreamTx,
    pub state: &'a AppState,
    pub message_id: &'a str,
    pub trace_id: Option<&'a str>,
    pub scoped_message_id: Option<&'a str>,
}

impl<'a> PlannerUiTarget<'a> {
    pub fn emit_phase_start(&self) {
        emit(
            self.stream,
            StreamEvent::AssistantJsonPartial {
                message_id: self.message_id.to_string(),
                thoughts: Some(PLANNER_PHASE_THOUGHTS.into()),
                headline: None,
                tool_name: None,
                response_text: None,
                trace_id: trace_id_opt(self.trace_id),
                scoped_message_id: trace_id_opt(self.scoped_message_id),
            },
        );
    }

    pub fn emit_tool_start(&self, tc: &ToolCall, history: Option<&mut Vec<ChatMessage>>) {
        let args = parse_tool_call_arguments(&tc.arguments);
        let display = self.state.tools.format_display(&tc.name, &args);
        let (display_label, display_summary) = tool_display_stream_fields(&display);
        let label = display_label.clone();
        let summary = display_summary.clone();
        if let Some(history) = history {
            upsert_assistant_tool_call(
                history,
                self.message_id,
                &ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    status: "pending".into(),
                    result: None,
                    error: None,
                    duration_ms: None,
                    risk_level: self
                        .state
                        .tools
                        .tool_risk_level_for_invocation(&tc.name, &args)
                        .or(Some("low".into())),
                    display_label: display_label.clone(),
                    display_summary: display_summary.clone(),
                },
            );
            patch_assistant_tool_call_display(history, self.message_id, &tc.id, &display);
        }
        emit(
            self.stream,
            StreamEvent::ToolCallStart {
                message_id: self.message_id.to_string(),
                tool_call: ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                    status: "pending".into(),
                    result: None,
                    error: None,
                    duration_ms: None,
                    risk_level: self
                        .state
                        .tools
                        .tool_risk_level_for_invocation(&tc.name, &args)
                        .or(Some("low".into())),
                    display_label,
                    display_summary,
                },
                trace_id: trace_id_opt(self.trace_id),
                scoped_message_id: trace_id_opt(self.scoped_message_id),
            },
        );
        emit(
            self.stream,
            StreamEvent::ToolCallStatus {
                message_id: self.message_id.to_string(),
                tool_call_id: tc.id.clone(),
                status: "running".into(),
                result: None,
                error: None,
                duration_ms: None,
                display_label: label,
                display_summary: summary,
                trace_id: trace_id_opt(self.trace_id),
                scoped_message_id: trace_id_opt(self.scoped_message_id),
            },
        );
    }

    pub fn emit_tool_complete(
        &self,
        tc: &ToolCall,
        args: &Value,
        outcome: &PlannerToolOutcome,
        duration_ms: u64,
        ok: bool,
        history: Option<&mut Vec<ChatMessage>>,
    ) {
        let display = self.state.tools.format_display(&tc.name, args);
        let (display_label, display_summary) = tool_display_stream_fields(&display);
        let status = if ok { "success" } else { "failed" };
        let preview = if outcome.tool_result.len() > 800 {
            format!("{}…", outcome.tool_result.chars().take(800).collect::<String>())
        } else {
            outcome.tool_result.clone()
        };
        let error = if ok {
            None
        } else {
            Some(preview.clone())
        };
        if let Some(history) = history {
            patch_assistant_tool_call_outcome(
                history,
                self.message_id,
                &tc.id,
                status,
                if ok { Some(preview.as_str()) } else { None },
                error.as_deref(),
                Some(duration_ms),
                display_label.as_deref(),
                display_summary.as_deref(),
            );
        }
        emit(
            self.stream,
            StreamEvent::ToolCallStatus {
                message_id: self.message_id.to_string(),
                tool_call_id: tc.id.clone(),
                status: status.into(),
                result: if ok { Some(preview) } else { None },
                error,
                duration_ms: Some(duration_ms),
                display_label,
                display_summary,
                trace_id: trace_id_opt(self.trace_id),
                scoped_message_id: trace_id_opt(self.scoped_message_id),
            },
        );
    }

    pub fn emit_phase_complete(&self, history: Option<&mut Vec<ChatMessage>>) {
        if let Some(history) = history {
            if let Some(msg) = history.iter_mut().find(|m| m.id == self.message_id) {
                msg.thoughts = None;
            }
        }
        emit(
            self.stream,
            StreamEvent::AssistantJsonPartial {
                message_id: self.message_id.to_string(),
                thoughts: Some(String::new()),
                headline: None,
                tool_name: None,
                response_text: None,
                trace_id: trace_id_opt(self.trace_id),
                scoped_message_id: trace_id_opt(self.scoped_message_id),
            },
        );
    }
}

/// Keep planner UI shell in transcript (tool cards) but omit from lead LLM context.
pub fn exclude_ui_shell_from_lead_context(history: &mut [ChatMessage], message_id: &str) {
    let id = message_id.trim();
    if id.is_empty() {
        return;
    }
    let Some(msg) = history.iter_mut().find(|m| m.id == id) else {
        return;
    };
    crate::message_context::mark_excluded(msg, crate::models::ExcludedReason::PlannerUiShell);
}

fn upsert_assistant_tool_call(
    history: &mut Vec<ChatMessage>,
    message_id: &str,
    incoming: &ToolCall,
) {
    let Some(msg) = history.iter_mut().find(|m| m.id == message_id) else {
        return;
    };
    let tcs = msg.tool_calls.get_or_insert_with(Vec::new);
    if let Some(existing) = tcs.iter_mut().find(|t| t.id == incoming.id) {
        existing.name = incoming.name.clone();
        existing.arguments = incoming.arguments.clone();
        existing.status = incoming.status.clone();
        existing.risk_level = incoming.risk_level.clone();
        if incoming.display_label.is_some() {
            existing.display_label = incoming.display_label.clone();
        }
        if incoming.display_summary.is_some() {
            existing.display_summary = incoming.display_summary.clone();
        }
    } else {
        tcs.push(incoming.clone());
    }
}
