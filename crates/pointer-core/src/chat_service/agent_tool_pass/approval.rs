//! Tool approval gate (manual mode / elevated terminal).

use crate::models::{StreamEvent, ToolCall};
use anyhow::Result;
use tokio::sync::oneshot;

use super::super::emit::{emit, trace_id_opt};
use super::types::ToolPassContext;

pub(super) async fn run_approval_gate(
    ctx: &mut ToolPassContext<'_>,
    tc: &ToolCall,
    tool_id: &str,
    args_value: &serde_json::Value,
    trace_id: Option<&str>,
) -> Result<bool> {
    let elevated_terminal =
        tool_id == "terminal" && crate::tools::terminal::terminal_requests_elevation(args_value);
    let requires_approval = elevated_terminal
        || tool_id == "video_generate"
        || (ctx.tool_approval_mode == "manual"
            && ctx
                .session
                .state
                .tools
                .tool_invocation_needs_approval(tool_id, args_value));
    if !requires_approval {
        return Ok(true);
    }

    let run_id = ctx
        .lead
        .as_ref()
        .map(|l| l.run_id)
        .or_else(|| ctx.sub.as_ref().map(|s| s.instance_scope.run_id.as_str()))
        .unwrap_or_default();
    let conversation_id = ctx.session.conversation_id;
    let trace_bus = ctx.session.state.trace_bus.clone();
    let mut approval_span = crate::observability::TraceEvent::new(
        run_id,
        uuid::Uuid::new_v4().to_string(),
        crate::observability::SpanKind::Approval,
        "approval",
    );
    approval_span.parent_span_id = Some("run-root".to_string());
    approval_span.run_id = run_id.to_string();
    approval_span.conversation_id = conversation_id.to_string();

    let scoped_message_id = ctx.sub.as_ref().map(|s| s.scoped_message_id.as_str());

    emit(
        ctx.session.stream,
        StreamEvent::ToolCallStatus {
            message_id: ctx.message_id.clone(),
            tool_call_id: tc.id.clone(),
            status: "pending_approval".into(),
            result: None,
            error: None,
            duration_ms: None,
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(trace_id),
            scoped_message_id: trace_id_opt(scoped_message_id),
        },
    );
    let (atx, arx) = oneshot::channel::<bool>();
    ctx.session
        .state
        .approvals
        .lock()
        .insert(tc.id.clone(), atx);
    let approved = tokio::select! {
        _ = ctx.session.cancel.cancelled() => {
            ctx.session.state.approvals.lock().remove(&tc.id);
            false
        }
        v = arx => v.unwrap_or(false),
    };
    if approved {
        approval_span.status = crate::observability::SpanStatus::Ok;
    } else {
        approval_span.status = crate::observability::SpanStatus::Cancelled;
    }
    approval_span.end();
    trace_bus.emit(approval_span);
    if approved {
        return Ok(true);
    }
    let err = "用户已拒绝该工具调用".to_string();
    emit(
        ctx.session.stream,
        StreamEvent::ToolCallStatus {
            message_id: ctx.message_id.clone(),
            tool_call_id: tc.id.clone(),
            status: "rejected".into(),
            result: None,
            error: Some(err.clone()),
            duration_ms: None,
            display_label: None,
            display_summary: None,
            trace_id: trace_id_opt(trace_id),
            scoped_message_id: trace_id_opt(scoped_message_id),
        },
    );
    super::super::util::push_tool_result(
        ctx.transcript.history,
        ctx.session.conversation_id,
        &ctx.message_id,
        &tc.id,
        &err,
        &ctx.persist,
    );
    Ok(false)
}
