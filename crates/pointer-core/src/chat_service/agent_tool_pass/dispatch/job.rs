//! `job` tool: list / status / await / cancel JobSupervisor entries.

use crate::tools::job::{parse_job_args, JobAction};
use crate::tools::parallel::ParallelLimits;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;

pub(super) async fn dispatch_job(
    state: &AppState,
    conversation_id: &str,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let parsed = parse_job_args(&args_value).map_err(|e| anyhow::anyhow!(e))?;
    let slot_cap = ParallelLimits::from_settings(&state.effective_settings()).max_parallel_sub_agents;
    let slot_cap = crate::chat_service::job_supervisor::JobSupervisor::slot_cap_from(slot_cap);
    let running_count = state.jobs.running_count_for_conversation(conversation_id);
    match parsed.action {
        JobAction::List => {
            let jobs = state.jobs.list(conversation_id, true);
            log::info!(
                "job tool: list conversation_id={conversation_id} count={} running_count={running_count} slot_cap={slot_cap}",
                jobs.len()
            );
            Ok((
                serde_json::to_string(&json!({
                    "jobs": jobs,
                    "runningCount": running_count,
                    "slotCap": slot_cap,
                }))?,
                true,
                None,
            ))
        }
        JobAction::Status => {
            let job_id = parsed
                .job_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("status requires jobId"))?;
            match state.jobs.status(conversation_id, job_id) {
                Ok(item) => {
                    log::info!(
                        "job tool: status conversation_id={conversation_id} job_id={job_id} status={}",
                        item.status
                    );
                    Ok((serde_json::to_string(&item)?, true, None))
                }
                Err(err) => {
                    log::warn!(
                        "job tool: status failed conversation_id={conversation_id} job_id={job_id}: {err}"
                    );
                    Ok((format!("ERROR: {err}"), false, Some(err)))
                }
            }
        }
        JobAction::Await => {
            let mode = crate::chat_service::job_supervisor::AwaitMode::parse(parsed.mode.as_deref());
            let ids = if parsed.job_ids.is_empty() {
                None
            } else {
                Some(parsed.job_ids.clone())
            };
            log::info!(
                "job tool: await start conversation_id={conversation_id} mode={:?} ids={:?} timeout_ms={:?}",
                mode,
                ids,
                parsed.timeout_ms
            );
            let result = state
                .jobs
                .await_jobs(
                    conversation_id,
                    ids,
                    mode,
                    parsed.timeout(),
                    cancel,
                    slot_cap,
                )
                .await;
            log::info!(
                "job tool: await done conversation_id={conversation_id} timed_out={} returned={} still_running={}",
                result.timed_out,
                result.jobs.len(),
                result.running.len()
            );
            Ok((serde_json::to_string(&result)?, true, None))
        }
        JobAction::Cancel => {
            let ids = if parsed.job_ids.is_empty() {
                None
            } else {
                Some(parsed.job_ids.as_slice())
            };
            let cancelled = state.jobs.cancel_ids(conversation_id, ids);
            log::info!(
                "job tool: cancel conversation_id={conversation_id} count={}",
                cancelled.len()
            );
            Ok((
                serde_json::to_string(&json!({
                    "cancelled": cancelled,
                    "runningCount": state.jobs.running_count_for_conversation(conversation_id),
                    "slotCap": slot_cap,
                }))?,
                true,
                None,
            ))
        }
    }
}
