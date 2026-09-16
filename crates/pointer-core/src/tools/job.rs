//! `job` tool: list / status / await / cancel background jobs.
//! Executed by the chat runtime (await is async), not via sync `ToolRegistry::invoke`.

use crate::tools::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

const DOC: &str = include_str!("prompts/job.md");
const DOC_SOURCE: &str = "tools/prompts/job.md";
const DEFAULT_TIMEOUT_MS: u64 = 30 * 60 * 1000;

pub fn register_all(reg: &ToolRegistry) {
    let handler: ToolHandler = Arc::new(|_args: Value| -> Result<String> {
        Err(anyhow!(
            "job is executed by the chat runtime, not synchronous invoke"
        ))
    });
    reg.register(
        ToolEntry::new("job", DOC_SOURCE, "low", false, DOC.trim(), handler)
            .with_subagent_inheritance(false),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobAction {
    List,
    Status,
    Await,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct JobToolArgs {
    pub action: JobAction,
    pub job_id: Option<String>,
    pub job_ids: Vec<String>,
    pub mode: Option<String>,
    pub timeout_ms: Option<u64>,
}

impl JobToolArgs {
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout_ms.map(Duration::from_millis)
    }

    pub fn default_timeout() -> Duration {
        Duration::from_millis(DEFAULT_TIMEOUT_MS)
    }
}

pub fn parse_job_args(args: &Value) -> Result<JobToolArgs, String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing or empty action".to_string())?;
    let action = match action {
        "list" => JobAction::List,
        "status" => JobAction::Status,
        "await" => JobAction::Await,
        "cancel" => JobAction::Cancel,
        other => return Err(format!("unknown action `{other}`")),
    };
    let job_id = args
        .get("jobId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut job_ids: Vec<String> = args
        .get("jobIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if let Some(id) = job_id.clone() {
        if !job_ids.iter().any(|x| x == &id) {
            job_ids.push(id);
        }
    }
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let timeout_ms = args.get("timeoutMs").and_then(|v| {
        v.as_u64().or_else(|| {
            v.as_f64()
                .and_then(|n| if n >= 0.0 { Some(n as u64) } else { None })
        })
    });
    if action == JobAction::Status && job_id.is_none() {
        return Err("status requires jobId".into());
    }
    Ok(JobToolArgs {
        action,
        job_id,
        job_ids,
        mode,
        timeout_ms,
    })
}

/// True when a tool result is a still-running job handle (`kind` + `jobId`).
/// Terminal stdout JSON and subagent worker bodies must not match.
pub fn is_running_job_handle(result: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(result) else {
        return false;
    };
    let job_id = v.get("jobId").and_then(|x| x.as_str()).unwrap_or("").trim();
    let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");
    let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("").trim();
    !job_id.is_empty()
        && status == "running"
        && (kind == "subagent" || kind == "terminal")
        && v.get("stdout").is_none()
        && v.get("exitCode").is_none()
        && v.get("content").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_await_any_default() {
        let parsed = parse_job_args(&json!({ "action": "await" })).unwrap();
        assert_eq!(parsed.action, JobAction::Await);
        assert!(parsed.job_ids.is_empty());
        assert!(parsed.mode.is_none());
    }

    #[test]
    fn parse_status_requires_id() {
        assert!(parse_job_args(&json!({ "action": "status" })).is_err());
        let parsed = parse_job_args(&json!({ "action": "status", "jobId": "job_1" })).unwrap();
        assert_eq!(parsed.job_ids, vec!["job_1".to_string()]);
    }

    #[test]
    fn running_handle_is_not_terminal_stdout() {
        assert!(is_running_job_handle(
            r#"{"jobId":"job_1","status":"running","kind":"terminal"}"#
        ));
        assert!(is_running_job_handle(
            r#"{"jobId":"job_1","status":"running","kind":"subagent"}"#
        ));
        assert!(!is_running_job_handle(
            r#"{"jobId":"job_1","status":"completed","kind":"terminal"}"#
        ));
        assert!(!is_running_job_handle(
            r#"{"exitCode":0,"success":true,"stdout":"hi"}"#
        ));
        assert!(!is_running_job_handle(r#"{"content":"worker markdown"}"#));
    }
}
