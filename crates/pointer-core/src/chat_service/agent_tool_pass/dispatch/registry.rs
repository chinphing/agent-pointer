//! Default registry invoke with file/computer profile guards.

use crate::agents::computer::ComputerTierGuard;
use crate::agents::{AgentProfile, FileToolLeadProfileGuard};
use crate::tools::file::{resolve_writable_path, ConversationWorkspaceGuard};
use std::path::Path;

use super::super::super::app_state::{AppState, ToolExecutionScope};
use super::super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult};

/// Resolve workspace root with session sandbox fallback.
///
/// Mirrors `resolve_terminal_session_workspace` so that file tools have the
/// same fallback behavior as terminal when no user-picked workspace exists.
pub(super) fn resolve_workspace_root(
    conversation_id: &str,
    session_user_id: &str,
    workspace_root: &str,
) -> String {
    let trimmed = workspace_root.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    // Try conversation store (user-picked workspace persisted across runs)
    if let Ok(store) = crate::conversation_store::global_store() {
        if let Ok(ws) = store.workspace_root(conversation_id) {
            if !ws.trim().is_empty() {
                log::info!(
                    "registry: workspace from conversation store conversation_id={conversation_id}: {ws}"
                );
                return ws.trim().to_string();
            }
        }
    }
    // Fall back to session sandbox
    match crate::session_sandbox::SessionSandbox::ensure_default(conversation_id, session_user_id) {
        Ok(path) => {
            let ws = path.display().to_string();
            log::info!(
                "registry: workspace from session sandbox conversation_id={conversation_id}: {ws}"
            );
            ws
        }
        Err(e) => {
            log::warn!(
                "registry: failed to ensure session sandbox conversation_id={conversation_id}: {e:#}"
            );
            String::new()
        }
    }
}

pub(super) async fn dispatch_registry_invoke(
    state: &AppState,
    conversation_id: &str,
    tool_id: &str,
    args_value: serde_json::Value,
    workspace_root: &str,
    lead: Option<&LeadToolPassConfig<'_>>,
    sub: Option<&SubToolPassConfig<'_>>,
    execution_scope: ToolExecutionScope,
) -> ToolExecResult {
    let file_profile = lead
        .map(|l| l.file_tool_lead_for_invoke.clone())
        .or_else(|| sub.map(|s| s.active.def.profile.clone()))
        .unwrap_or(AgentProfile::General);
    dispatch_registry_invoke_with_profile(
        state,
        conversation_id,
        tool_id,
        args_value,
        workspace_root,
        file_profile,
        execution_scope,
    )
    .await
}

pub(super) async fn dispatch_registry_invoke_with_profile(
    state: &AppState,
    conversation_id: &str,
    tool_id: &str,
    args_value: serde_json::Value,
    workspace_root: &str,
    file_profile: AgentProfile,
    execution_scope: ToolExecutionScope,
) -> ToolExecResult {
    let session_user_id = state
        .session_index
        .session_user_id(conversation_id)
        .unwrap_or_default();
    let resolved_workspace =
        resolve_workspace_root(conversation_id, &session_user_id, workspace_root);
    let _write_guard = if matches!(tool_id, "file_write" | "file_edit") {
        let path = args_value
            .get("path")
            .or_else(|| args_value.get("file"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow::anyhow!("缺少 path"))?;
        let target =
            resolve_writable_path(Path::new(&resolved_workspace), path).map_err(|error| {
                log::warn!(
                    "file_lock: path resolution failed tool={} path={} {} error={error:#}",
                    tool_id,
                    path,
                    execution_scope.log_fields()
                );
                error
            })?;
        let (guard, waited, normalized) =
            state
                .file_write_locks
                .lock_path(&target)
                .await
                .map_err(|error| {
                    log::warn!(
                        "file_lock: acquisition failed tool={} path={} {} error={error:#}",
                        tool_id,
                        target.display(),
                        execution_scope.log_fields()
                    );
                    error
                })?;
        if !waited.is_zero() {
            log::info!(
                "file_lock: acquired tool={} path={} wait_us={} {}",
                tool_id,
                normalized.display(),
                waited.as_micros(),
                execution_scope.log_fields()
            );
        }
        Some(guard)
    } else {
        None
    };
    let _file_tool_profile_guard = FileToolLeadProfileGuard::enter(file_profile.clone());

    // Re-establish thread-local workspace root: tokio may have moved us to a
    // different worker thread since `ConversationWorkspaceGuard::enter` was set
    // in `run_chat_inner`, so the thread-local is empty here.
    let _workspace_guard = ConversationWorkspaceGuard::enter(resolved_workspace.clone());
    let _work_dir_guard =
        crate::session_work_dir_env::SessionWorkDirGuard::enter(resolved_workspace);
    let _session_user_guard = crate::session_user_env::SessionUserIdGuard::enter(session_user_id);
    let _turn_baseline_guard = if matches!(tool_id, "file_write" | "file_edit") {
        let turn_id = crate::turn_file_baseline::resolve_active_turn_id(conversation_id);
        Some(crate::turn_file_baseline::TurnBaselineGuard::enter(
            conversation_id.to_string(),
            turn_id,
        ))
    } else {
        None
    };
    let _tier_guard = if file_profile == AgentProfile::Computer {
        Some(ComputerTierGuard::enter(
            state.computer_state.tier_for_conversation(conversation_id),
        ))
    } else {
        None
    };
    state
        .tools
        .invoke(tool_id, args_value)
        .map(|out| (out, true, None))
}
