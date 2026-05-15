use crate::agents::{DEFAULT_AGENT_ID, SUPERVISOR_AGENT_ID, AGENT_MODE_SUPERVISOR};

pub(crate) fn apply_session_agent_model_defaults(
    settings: &mut crate::models::ModelSettings,
    effective_agent_mode: &str,
) {
    let mode = effective_agent_mode.trim();
    let key = if mode == AGENT_MODE_SUPERVISOR {
        SUPERVISOR_AGENT_ID.to_string()
    } else {
        let id = settings.lead_agent_id.trim();
        if id.is_empty() {
            DEFAULT_AGENT_ID.to_string()
        } else {
            id.to_string()
        }
    };
    if let Some(pref) = settings.agent_default_models.get(&key) {
        if !pref.provider_id.trim().is_empty() {
            settings.active_provider_id = pref.provider_id.trim().to_string();
        }
        if !pref.model.trim().is_empty() {
            settings.model = pref.model.trim().to_string();
        }
    }
}
