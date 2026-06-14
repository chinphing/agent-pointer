//! Skill import side effects (persist enabled ids + stream event).

use crate::models::StreamEvent;

use super::super::super::app_state::AppState;
use super::super::super::emit::emit;
use super::super::super::StreamTx;
use super::super::types::{LeadToolPassConfig, ToolExecResult};

pub(super) fn dispatch_skill_import(
    stream: &StreamTx,
    state: &AppState,
    conversation_id: &str,
    tool_id: &str,
    args_value: serde_json::Value,
    lead: Option<&mut LeadToolPassConfig<'_>>,
) -> ToolExecResult {
    let auto_enable = args_value
        .get("auto_enable")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let out = state.tools.invoke(tool_id, args_value)?;
    let result: crate::models::SkillImportResult = serde_json::from_str(&out)
        .map_err(|e| anyhow::anyhow!("skill_import 结果解析失败: {e}"))?;
    let imported_ids: Vec<String> = result.imported.iter().map(|s| s.id.clone()).collect();
    log::info!(
        "skill_import: conversation_id={conversation_id} imported={} skipped={}",
        imported_ids.len(),
        result.skipped.len()
    );
    let enabled_ids = if auto_enable {
        if let Some(lead_cfg) = lead {
            for id in &imported_ids {
                if !lead_cfg.enabled_skill_ids.contains(id) {
                    lead_cfg.enabled_skill_ids.push(id.clone());
                }
            }
            let ids = lead_cfg.enabled_skill_ids.clone();
            let mut user = state.load_user_settings();
            user.enabled_skill_ids = ids.clone();
            if let Err(err) = state.save_user_settings(&user) {
                log::warn!("skill_import: persist enabled_skill_ids failed: {err}");
            }
            Some(ids)
        } else {
            None
        }
    } else {
        None
    };
    emit(
        stream,
        StreamEvent::SkillsUpdated {
            conversation_id: conversation_id.to_string(),
            imported_ids: imported_ids.clone(),
            enabled_ids,
        },
    );
    Ok((out, true, None))
}
