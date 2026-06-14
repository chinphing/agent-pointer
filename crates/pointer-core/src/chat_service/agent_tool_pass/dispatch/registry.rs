//! Default registry invoke with file/computer profile guards.

use crate::agents::computer::ComputerTierGuard;
use crate::agents::{AgentProfile, FileToolLeadProfileGuard};

use super::super::super::app_state::AppState;
use super::super::types::{LeadToolPassConfig, SubToolPassConfig, ToolExecResult};

pub(super) fn dispatch_registry_invoke(
    state: &AppState,
    conversation_id: &str,
    tool_id: &str,
    args_value: serde_json::Value,
    lead: Option<&LeadToolPassConfig<'_>>,
    sub: Option<&SubToolPassConfig<'_>>,
) -> ToolExecResult {
    let file_profile = lead
        .map(|l| l.file_tool_lead_for_invoke.clone())
        .or_else(|| sub.map(|s| s.def.profile.clone()))
        .unwrap_or(AgentProfile::General);
    let _file_tool_profile_guard = FileToolLeadProfileGuard::enter(file_profile.clone());
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
