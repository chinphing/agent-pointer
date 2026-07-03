//! Block **`run_subagent` → computer** until the user picks a monitor (same rules as Computer lead send).

use crate::agents::computer::screen;
use crate::models::StreamEvent;
use anyhow::{anyhow, Result};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::app_state::AppState;
use super::computer_monitor_follow::{
    computer_subagent_monitor_flow_required, ensure_primary_monitor_when_auto,
};
use super::emit::emit;
use super::StreamTx;

/// Whether monitor selection UI should run (computer agent UI + platform setting + user pref).
pub fn computer_monitor_picker_enabled(
    state: &AppState,
    settings: &crate::models::ModelSettings,
) -> bool {
    super::computer_monitor_follow::computer_monitor_manual_pick_required(state, settings)
}

/// Resolve monitor for a computer sub-agent; may block on UI pick.
pub async fn ensure_computer_monitor_for_subagent(
    stream: &StreamTx,
    state: &AppState,
    settings: &crate::models::ModelSettings,
    conversation_id: &str,
    message_id: &str,
    tool_call_id: &str,
    cancel: &CancellationToken,
) -> Result<()> {
    if !computer_subagent_monitor_flow_required(state, settings) {
        if settings.computer_auto_switch_monitor {
            ensure_primary_monitor_when_auto(stream, state, settings, conversation_id)?;
        } else if !state
            .computer_state
            .is_monitor_selection_done(conversation_id)
        {
            let monitors = screen::list_monitors().map_err(|e| anyhow!("list monitors: {e}"))?;
            if monitors.len() == 1 {
                let id = monitors[0].id.clone();
                state
                    .computer_state
                    .set_conversation_monitor(conversation_id, Some(id.clone()));
                emit_monitor_updated(stream, conversation_id, Some(id));
            }
        }
        log::info!(
            "computer_monitor_pick: skipped (flow disabled) conversation_id={conversation_id}"
        );
        return Ok(());
    }

    let monitors = screen::list_monitors().map_err(|e| anyhow!("list monitors: {e}"))?;
    if monitors.is_empty() {
        return Err(anyhow!("未检测到可用屏幕"));
    }

    log::info!(
        "computer_monitor_pick: waiting for frontend flow conversation_id={conversation_id} monitors={}",
        monitors.len()
    );
    emit(
        stream,
        StreamEvent::ComputerMonitorPickRequired {
            conversation_id: conversation_id.to_string(),
            message_id: message_id.to_string(),
            tool_call_id: tool_call_id.to_string(),
            monitors,
        },
    );

    let (tx, rx) = oneshot::channel::<Result<(), String>>();
    state
        .monitor_picks
        .lock()
        .insert(conversation_id.to_string(), tx);

    let outcome = tokio::select! {
        _ = cancel.cancelled() => {
            state.monitor_picks.lock().remove(conversation_id);
            Err(anyhow!("已停止生成"))
        }
        v = rx => match v {
            Ok(Ok(())) => Ok(()),
            Ok(Err(msg)) => Err(anyhow!("{msg}")),
            Err(_) => Err(anyhow!("屏幕选择已取消")),
        },
    };

    outcome
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::computer_monitor_follow::computer_subagent_monitor_flow_required;

    #[test]
    fn picker_disabled_when_platform_flag_off() {
        let state = AppState::new();
        let mut settings = state.effective_settings();
        settings.computer_show_monitor_picker = false;
        assert!(!computer_monitor_picker_enabled(&state, &settings));
        assert!(!computer_subagent_monitor_flow_required(&state, &settings));
    }

    #[test]
    fn manual_picker_disabled_when_auto_switch_on() {
        let state = AppState::new();
        let mut settings = state.effective_settings();
        settings.computer_auto_switch_monitor = true;
        assert!(!computer_monitor_picker_enabled(&state, &settings));
    }

    #[test]
    fn subagent_flow_still_required_when_auto_switch_on() {
        let state = AppState::new();
        let mut settings = state.effective_settings();
        settings.computer_auto_switch_monitor = true;
        assert!(computer_subagent_monitor_flow_required(&state, &settings));
    }
}
