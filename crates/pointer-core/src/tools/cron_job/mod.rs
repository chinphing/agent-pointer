//! `cron_job` tool — manage scheduled agent runs from chat (general agent only).

pub mod schedule;

use crate::conversation_store::cron_jobs::{self, CronJobView, NewCronJob};
use crate::conversation_store::ConversationStore;
use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, TimeZone};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::sync::Arc;

const CRON_JOB_MD: &str = include_str!("prompts/cron_job.md");
const CRON_JOB_DOC_SOURCE: &str = "tools/cron_job/prompts/cron_job.md";

/// Optional deliver-spec validator installed by the IM outbound bridge
/// (reads live channel bindings). When unset, create skips binding checks.
type DeliverValidator = Arc<dyn Fn(Option<&str>) -> Result<(), String> + Send + Sync>;

static DELIVER_VALIDATOR: RwLock<Option<DeliverValidator>> = RwLock::new(None);

/// Install (or clear) the deliver validator used by `cron_job` create.
pub fn set_deliver_validator(validator: Option<DeliverValidator>) {
    let installed = validator.is_some();
    *DELIVER_VALIDATOR.write() = validator;
    if installed {
        log::info!("cron_job: deliver validator installed");
    } else {
        log::info!("cron_job: deliver validator cleared");
    }
}

fn validate_deliver_arg(deliver: Option<&str>) -> Result<()> {
    let guard = DELIVER_VALIDATOR.read();
    let Some(v) = guard.as_ref() else {
        return Ok(());
    };
    v(deliver).map_err(|e| anyhow!(e))
}

pub fn register(reg: &ToolRegistry, store: Arc<ConversationStore>) {
    let doc = CRON_JOB_MD.trim();
    let st = store.clone();
    let handler: ToolHandler =
        Arc::new(move |args: Value| -> Result<String> { dispatch(&st, &args) });

    reg.register(
        ToolEntry::new("cron_job", CRON_JOB_DOC_SOURCE, "low", false, doc, handler)
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "list", "enable", "disable", "delete"],
                        "description": "Operation. Default create when prompt_text and schedule are provided."
                    },
                    "prompt_text": {
                        "type": "string",
                        "description": "Agent instruction for each scheduled run (create)."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "When to run: one-shot (30m, 2h, 1d, or ISO 2026-07-22T09:00:00), friendly recurring (daily@9:30, every_5_minutes), or raw 6-field cron (0 30 9 * * *)."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Cron job id for enable, disable, or delete."
                    },
                    "label": {
                        "type": "string",
                        "description": "Optional short name; defaults to the first line of prompt_text."
                    },
                    "deliver": {
                        "type": "string",
                        "description": "Optional IM push after each run. Prefer channel names only: feishu, dingtalk, wecom, weixin, comma-separated, or all. The channel must already be bound (user sent a private message to Pointer on that channel). Omit / empty = no push."
                    }
                }
            }))
            .with_subagent_inheritance(false),
    );
}

pub fn plan_includes_cron_job(allowed_tool_names: &[String]) -> bool {
    allowed_tool_names.iter().any(|n| n == "cron_job")
}

fn dispatch(store: &ConversationStore, args: &Value) -> Result<String> {
    let action = infer_action(args);
    match action.as_str() {
        "create" => create_job(store, args),
        "list" => list_jobs(store),
        "enable" => set_enabled(store, args, true),
        "disable" => set_enabled(store, args, false),
        "delete" => delete_job(store, args),
        other => Err(anyhow!(
            "Unknown action '{other}'. Use create, list, enable, disable, or delete."
        )),
    }
}

fn infer_action(args: &Value) -> String {
    if let Some(a) = args.get("action").and_then(|v| v.as_str()) {
        let t = a.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let has_create = args
        .get("prompt_text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
        && args
            .get("schedule")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some();
    if has_create {
        "create".into()
    } else {
        "list".into()
    }
}

fn create_job(store: &ConversationStore, args: &Value) -> Result<String> {
    let prompt = args
        .get("prompt_text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("prompt_text is required for create"))?;
    let schedule_raw = args
        .get("schedule")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("schedule is required for create"))?;

    let parsed = schedule::parse_schedule(schedule_raw)?;
    let (cron_expr, schedule_kind, next_override) = match &parsed {
        schedule::ParsedSchedule::Recurring { cron_expr } => {
            if cron_jobs::next_run_ms_now(cron_expr).is_none() {
                return Err(anyhow!("invalid cron expression after parse: {cron_expr}"));
            }
            (cron_expr.clone(), cron_jobs::SCHEDULE_KIND_CRON, None)
        }
        schedule::ParsedSchedule::Once { fire_at_ms } => (
            schedule::ONCE_CRON_PLACEHOLDER.to_string(),
            cron_jobs::SCHEDULE_KIND_ONCE,
            Some(*fire_at_ms),
        ),
    };

    let conv_id = args
        .get("_conversation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let (lead_agent_id, agent_mode) = resolve_agent_defaults(store, conv_id);

    let label = args
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| auto_label(prompt));

    let deliver = args
        .get("deliver")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    validate_deliver_arg(deliver.as_deref())?;
    let deliver = deliver.map(|d| normalize_deliver_local(&d));

    let job_id = gen_job_id();
    let job_id_ref = job_id.as_str();
    let label_ref = label.as_str();
    let lead_ref = lead_agent_id.as_str();
    let mode_ref = agent_mode.as_str();
    let deliver_ref = deliver.as_deref();

    let new = NewCronJob {
        id: job_id_ref,
        label: label_ref,
        cron_expr: &cron_expr,
        schedule_kind,
        schedule_raw: Some(schedule_raw),
        next_run_at_ms: next_override,
        conversation_id: "",
        prompt_text: prompt,
        agent_mode: Some(mode_ref),
        lead_agent_id: Some(lead_ref),
        enabled: true,
        deliver: deliver_ref,
    };

    let inserted = store.cron_jobs_insert(&new)?;
    if !inserted {
        return Err(anyhow!("failed to insert cron job (duplicate id)"));
    }

    let rec = store
        .cron_jobs_get(job_id_ref)?
        .ok_or_else(|| anyhow!("cron job vanished after insert"))?;
    let view = CronJobView::from_record(&rec);
    let schedule_desc = schedule::describe_job(
        &rec.schedule_kind,
        &rec.cron_expr,
        rec.schedule_raw.as_deref(),
        rec.next_run_at_ms,
    );
    let next_run = format_next_run(rec.next_run_at_ms);

    log::info!(
        "cron_job tool: created id={} label={} kind={} expr={} from conversation={}",
        job_id,
        label,
        schedule_kind,
        cron_expr,
        if conv_id.is_empty() {
            "(none)"
        } else {
            conv_id
        }
    );

    Ok(json!({
        "ok": true,
        "action": "create",
        "job": view,
        "scheduleDescription": schedule_desc,
        "nextRunAt": next_run,
        "hint": "The user can manage jobs in Settings → Automation, or ask you to list / enable / disable / delete them. One-shot jobs soft-complete after firing (kept disabled for history)."
    })
    .to_string())
}

fn list_jobs(store: &ConversationStore) -> Result<String> {
    let rows = store.cron_jobs_list_all()?;
    let jobs: Vec<Value> = rows
        .iter()
        .map(|r| {
            let view = CronJobView::from_record(r);
            json!({
                "job": view,
                "scheduleDescription": schedule::describe_job(
                    &r.schedule_kind,
                    &r.cron_expr,
                    r.schedule_raw.as_deref(),
                    r.next_run_at_ms,
                ),
                "nextRunAt": format_next_run(r.next_run_at_ms),
            })
        })
        .collect();
    Ok(json!({
        "ok": true,
        "action": "list",
        "count": jobs.len(),
        "jobs": jobs,
    })
    .to_string())
}

fn set_enabled(store: &ConversationStore, args: &Value, enabled: bool) -> Result<String> {
    let job_id = require_job_id(args)?;
    let ok = store.cron_jobs_set_enabled(&job_id, enabled)?;
    if !ok {
        return Err(anyhow!("cron job not found: {job_id}"));
    }
    let rec = store
        .cron_jobs_get(&job_id)?
        .ok_or_else(|| anyhow!("cron job not found after update: {job_id}"))?;
    let view = CronJobView::from_record(&rec);
    log::info!("cron_job tool: id={} enabled={enabled}", job_id);
    Ok(json!({
        "ok": true,
        "action": if enabled { "enable" } else { "disable" },
        "job": view,
        "scheduleDescription": schedule::describe_job(
            &rec.schedule_kind,
            &rec.cron_expr,
            rec.schedule_raw.as_deref(),
            rec.next_run_at_ms,
        ),
        "nextRunAt": format_next_run(rec.next_run_at_ms),
    })
    .to_string())
}

fn delete_job(store: &ConversationStore, args: &Value) -> Result<String> {
    let job_id = require_job_id(args)?;
    let ok = store.cron_jobs_delete(&job_id)?;
    if !ok {
        return Err(anyhow!("cron job not found: {job_id}"));
    }
    log::info!("cron_job tool: deleted id={job_id}");
    Ok(json!({
        "ok": true,
        "action": "delete",
        "jobId": job_id,
    })
    .to_string())
}

fn require_job_id(args: &Value) -> Result<String> {
    args.get("job_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("job_id is required for this action"))
}

fn resolve_agent_defaults(store: &ConversationStore, conv_id: &str) -> (String, String) {
    const DEFAULT_LEAD: &str = "general";
    const DEFAULT_MODE: &str = "single";
    if conv_id.is_empty() {
        return (DEFAULT_LEAD.into(), DEFAULT_MODE.into());
    }
    match store.load_meta(conv_id) {
        Ok(Some(meta)) => {
            let lead = if meta.lead_agent_id.trim().is_empty() {
                DEFAULT_LEAD.to_string()
            } else {
                meta.lead_agent_id
            };
            let mode = if meta.agent_mode.trim().is_empty() {
                DEFAULT_MODE.to_string()
            } else {
                meta.agent_mode
            };
            (lead, mode)
        }
        Ok(None) | Err(_) => (DEFAULT_LEAD.into(), DEFAULT_MODE.into()),
    }
}

fn auto_label(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return "定时任务".into();
    }
    const MAX: usize = 48;
    if line.chars().count() <= MAX {
        line.to_string()
    } else {
        format!("{}…", line.chars().take(MAX).collect::<String>())
    }
}

/// Lowercase bare channel names / `all`; keep explicit `channel:id` recipient casing.
fn normalize_deliver_local(deliver: &str) -> String {
    deliver
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|token| {
            if token.eq_ignore_ascii_case("all") {
                return "all".to_string();
            }
            if let Some((ch, rest)) = token.split_once(':') {
                format!("{}:{}", ch.trim().to_ascii_lowercase(), rest.trim())
            } else {
                token.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn gen_job_id() -> String {
    let short = uuid::Uuid::new_v4().simple().to_string();
    format!("cron-{}", &short[..12])
}

fn format_next_run(ms: Option<i64>) -> Value {
    match ms {
        Some(v) => {
            let local: DateTime<Local> = Local
                .timestamp_millis_opt(v)
                .single()
                .unwrap_or_else(Local::now);
            json!({
                "ms": v,
                "local": local.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
            })
        }
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_via_tool() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let out = dispatch(
            &store,
            &json!({
                "action": "create",
                "prompt_text": "Summarize inbox",
                "schedule": "daily@8:00"
            }),
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
        assert!(v.get("job").is_some());
    }

    #[test]
    fn create_once_via_tool_then_soft_complete() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::open_in_dir(dir.path()).unwrap();
        let out = dispatch(
            &store,
            &json!({
                "action": "create",
                "prompt_text": "Remind me to stretch",
                "schedule": "30m"
            }),
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
        let job = v.get("job").unwrap();
        assert_eq!(
            job.get("scheduleKind").and_then(|x| x.as_str()),
            Some("once")
        );
        let id = job.get("id").and_then(|x| x.as_str()).unwrap().to_string();
        store.cron_jobs_mark_ran(&id, chrono::Local::now()).unwrap();
        let rec = store.cron_jobs_get(&id).unwrap().unwrap();
        assert!(!rec.enabled);
        assert!(rec.next_run_at_ms.is_none());
        let enable_err = store.cron_jobs_set_enabled(&id, true).unwrap_err();
        assert!(enable_err.to_string().contains("cannot be re-enabled"));
    }
}
