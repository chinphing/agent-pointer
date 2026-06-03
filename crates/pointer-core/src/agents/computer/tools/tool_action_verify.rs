//! Sidecar flat tool `action_verify` for computer tier runtime.

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Max chars for `step_summary` (tool result + history).
const STEP_SUMMARY_MAX_CHARS: usize = 400;

/// Validate and accept action-verify sidecar payload.
pub fn execute_action_verify(args: &Value) -> Result<String> {
    let action_result = args
        .get("action_result")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("missing required parameter: action_result"))?
        .to_ascii_lowercase();
    if !matches!(action_result.as_str(), "pass" | "fail" | "pending" | "n/a") {
        return Err(anyhow!(
            "invalid action_result: expected one of pass|fail|pending|n/a"
        ));
    }
    let repetition_count = parse_repetition_count(args.get("repetition_count"))?;
    let failure_cause = args
        .get("failure_cause")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    if action_result == "fail" {
        let Some(cause) = failure_cause.as_deref() else {
            return Err(anyhow!(
                "missing required parameter on fail: failure_cause"
            ));
        };
        if !matches!(cause, "wrong_operation" | "precision_miss") {
            return Err(anyhow!(
                "invalid failure_cause: expected one of wrong_operation|precision_miss"
            ));
        }
    } else if failure_cause.is_some() {
        return Err(anyhow!(
            "failure_cause is only allowed when action_result=fail"
        ));
    }

    let step_summary = parse_step_summary(args.get("step_summary"))?;
    if action_result == "pass" {
        if step_summary.is_none() {
            return Err(anyhow!("missing required parameter on pass: step_summary"));
        }
    }

    Ok(match action_result.as_str() {
        "pending" => format!(
            "action_verify accepted: action_result=pending, repetition_count={repetition_count} — newest verifying row stays open until pass/fail/n/a"
        ),
        "pass" => {
            let summary = step_summary.expect("checked above");
            format!(
                "action_verify accepted: action_result=pass, repetition_count={repetition_count}, step_summary={summary} — host closes newest verifying row as verified - pass"
            )
        }
        "fail" => format!(
            "action_verify accepted: action_result=fail, repetition_count={repetition_count}, failure_cause={} — host closes newest verifying row as verified - {}",
            failure_cause.as_deref().unwrap_or("n/a"),
            failure_cause.as_deref().unwrap_or("fail")
        ),
        _ => format!(
            "action_verify accepted: action_result={action_result}, repetition_count={repetition_count} — host closes newest verifying row"
        ),
    })
}

fn parse_step_summary(v: Option<&Value>) -> Result<Option<String>> {
    let Some(v) = v else {
        return Ok(None);
    };
    let s = v
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("invalid step_summary: expected non-empty string"))?;
    Ok(Some(truncate_chars(s, STEP_SUMMARY_MAX_CHARS)))
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

fn parse_repetition_count(v: Option<&Value>) -> Result<u64> {
    let Some(v) = v else {
        return Ok(0);
    };
    match v {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| anyhow!("invalid repetition_count: expected non-negative integer")),
        Value::String(s) => s
            .trim()
            .parse::<u64>()
            .map_err(|_| anyhow!("invalid repetition_count: expected non-negative integer")),
        _ => Err(anyhow!(
            "invalid repetition_count: expected non-negative integer"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_requires_step_summary() {
        let err = execute_action_verify(&serde_json::json!({
            "action_result": "pass",
            "repetition_count": 0,
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("step_summary"));
    }

    #[test]
    fn pass_includes_step_summary_in_tool_result() {
        let out = execute_action_verify(&serde_json::json!({
            "action_result": "pass",
            "repetition_count": 0,
            "step_summary": "Toward 登录微信: 进入主界面，聊天列表可见"
        }))
        .unwrap();
        assert!(out.contains("step_summary="));
        assert!(out.contains("登录微信"));
    }

    #[test]
    fn fail_does_not_require_step_summary() {
        let out = execute_action_verify(&serde_json::json!({
            "action_result": "fail",
            "repetition_count": 3,
            "failure_cause": "precision_miss",
        }))
        .unwrap();
        assert!(out.contains("action_result=fail"));
        assert!(!out.contains("step_summary="));
    }

    #[test]
    fn pending_omits_step_summary() {
        let out = execute_action_verify(&serde_json::json!({
            "action_result": "pending",
            "repetition_count": 1
        }))
        .unwrap();
        assert!(out.contains("pending"));
        assert!(!out.contains("step_summary="));
    }

    #[test]
    fn rejects_fail_without_failure_cause() {
        let err = execute_action_verify(&serde_json::json!({
            "action_result": "fail",
            "repetition_count": 1
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("failure_cause"));
    }

    #[test]
    fn truncates_long_step_summary() {
        let long = "a".repeat(500);
        let out = execute_action_verify(&serde_json::json!({
            "action_result": "pass",
            "repetition_count": 0,
            "step_summary": long
        }))
        .unwrap();
        let start = out.find("step_summary=").expect("summary");
        let body = &out[start + "step_summary=".len()..];
        let end = body.find(" — ").unwrap_or(body.len());
        let summary = &body[..end];
        assert!(summary.chars().count() <= 401);
        assert!(summary.ends_with('…'));
    }
}
