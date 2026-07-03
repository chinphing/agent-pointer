//! Monitor selection and launch_app capture follow.

use crate::agents::computer::screen;
use crate::agents::computer::tools::tool_app_access;
use crate::chat_service::emit::emit;
use crate::models::{ModelSettings, StreamEvent};
use crate::chat_service::app_state::AppState;
use crate::chat_service::StreamTx;
use serde_json::Value;

fn computer_agent_show_monitor_picker(state: &AppState) -> bool {
    let Some(exec) = state.agents.get("computer") else {
        return false;
    };
    crate::agents::agent_ui::resolve_agent_ui(&exec.def()).show_computer_monitor_picker
}

/// Whether manual monitor picker should run (computer agent UI + platform setting + user pref).
pub fn computer_monitor_manual_pick_required(
    state: &AppState,
    settings: &ModelSettings,
) -> bool {
    if !settings.computer_show_monitor_picker {
        return false;
    }
    if settings.computer_auto_switch_monitor {
        return false;
    }
    computer_agent_show_monitor_picker(state)
}

/// Whether **`run_subagent` → computer** should block on the frontend monitor/permissions flow
/// (same gate as Computer lead send — includes auto-switch; macOS permissions run in UI).
pub fn computer_subagent_monitor_flow_required(
    state: &AppState,
    settings: &ModelSettings,
) -> bool {
    if !settings.computer_show_monitor_picker {
        return false;
    }
    computer_agent_show_monitor_picker(state)
}

/// Apply primary monitor when auto-switch mode is enabled (skip manual picker).
pub fn ensure_primary_monitor_when_auto(
    stream: &StreamTx,
    state: &AppState,
    settings: &ModelSettings,
    conversation_id: &str,
) -> anyhow::Result<()> {
    if !settings.computer_auto_switch_monitor {
        return Ok(());
    }
    if !computer_monitor_picker_enabled(state, settings) {
        return Ok(());
    }
    if state
        .computer_state
        .is_monitor_selection_done(conversation_id)
    {
        return Ok(());
    }
    let id = screen::primary_monitor_id()?;
    state
        .computer_state
        .set_conversation_monitor(conversation_id, Some(id.clone()));
    emit_monitor_updated(stream, conversation_id, Some(id));
    log::info!(
        "computer_monitor_pick: auto-switch mode selected primary monitor conversation_id={conversation_id}"
    );
    Ok(())
}

/// After a successful desktop tool, follow the active app window to its display when auto-switch is on.
pub fn maybe_auto_switch_capture_monitor_after_tool(
    stream: &StreamTx,
    state: &AppState,
    settings: &ModelSettings,
    conversation_id: &str,
    tool_id: &str,
    ok: bool,
    args: &Value,
) {
    if !ok || !settings.computer_auto_switch_monitor {
        return;
    }
    if !crate::agents::computer::is_desktop_post_delay_tool(tool_id) {
        return;
    }

    let new_id = if tool_id == "launch_app" {
        tool_app_access::monitor_id_for_launched_app(args)
    } else {
        crate::platform::app_access::monitor_id_for_frontmost_app()
    };

    let Some(new_id) = new_id else {
        log::warn!(
            "auto monitor switch: display unavailable conversation_id={conversation_id} tool={tool_id} app={}",
            args.get("app").and_then(|v| v.as_str()).unwrap_or("")
        );
        return;
    };

    let current = state
        .computer_state
        .conversation_monitor_id(conversation_id);
    if current.as_deref() == Some(new_id.as_str()) {
        log::info!(
            "auto monitor switch: already on monitor_id={new_id} conversation_id={conversation_id} tool={tool_id}"
        );
        return;
    }

    state
        .computer_state
        .set_conversation_monitor(conversation_id, Some(new_id.clone()));
    emit_monitor_updated(stream, conversation_id, Some(new_id.clone()));
    log::info!(
        "auto monitor switch: switched capture monitor conversation_id={conversation_id} tool={tool_id} monitor_id={new_id}"
    );
}

fn computer_monitor_picker_enabled(
    state: &AppState,
    settings: &ModelSettings,
) -> bool {
    if !settings.computer_show_monitor_picker {
        return false;
    }
    let Some(exec) = state.agents.get("computer") else {
        return false;
    };
    let def = exec.def();
    crate::agents::agent_ui::resolve_agent_ui(&def).show_computer_monitor_picker
}

fn emit_monitor_updated(stream: &StreamTx, conversation_id: &str, monitor_id: Option<String>) {
    emit(
        stream,
        StreamEvent::ComputerMonitorUpdated {
            conversation_id: conversation_id.to_string(),
            monitor_id,
        },
    );
}
