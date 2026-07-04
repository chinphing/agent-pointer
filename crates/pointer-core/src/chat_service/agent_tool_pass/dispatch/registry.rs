//! Default registry invoke with file/computer profile guards.

use crate::agents::computer::ComputerTierGuard;
use crate::agents::{AgentProfile, FileToolLeadProfileGuard};
use crate::tools::file::ConversationWorkspaceGuard;

use super::super::super::app_state::AppState;
use super::super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult};

pub(super) fn dispatch_registry_invoke(
    state: &AppState,
    conversation_id: &str,
    tool_id: &str,
    args_value: serde_json::Value,
    workspace_root: &str,
    lead: Option<&LeadToolPassConfig<'_>>,
    sub: Option<&SubToolPassConfig<'_>>,
) -> ToolExecResult {
    let file_profile = lead
        .map(|l| l.file_tool_lead_for_invoke.clone())
        .or_else(|| sub.map(|s| s.def.profile.clone()))
        .unwrap_or(AgentProfile::General);
    let _file_tool_profile_guard = FileToolLeadProfileGuard::enter(file_profile.clone());
    // Re-establish thread-local workspace root: tokio may have moved us to a
    // different worker thread since `ConversationWorkspaceGuard::enter` was set
    // in `run_chat_inner`, so the thread-local is empty here.
    let _workspace_guard = ConversationWorkspaceGuard::enter(workspace_root.to_string());
    let session_user_id = state
        .session_index
        .session_user_id(conversation_id)
        .unwrap_or_default();
    let _session_user_guard =
        crate::session_user_env::SessionUserIdGuard::enter(session_user_id);
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
