//! Sidecar signal tool for computer tier runtime.

use anyhow::{anyhow, Result};
use serde_json::Value;

/// Validate sidecar tier signal payload.
pub fn execute_tier_signal(args: &Value) -> Result<String> {
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
    Ok(format!(
        "Sidecar verify signal accepted: action_result={action_result}, repetition_count={repetition_count}, failure_cause={}",
        failure_cause.as_deref().unwrap_or("n/a")
    ))
}

fn parse_repetition_count(v: Option<&Value>) -> Result<u64> {
    let Some(v) = v else {
        // Be tolerant for missing field so sidecar does not fail the whole turn.
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
    fn accepts_valid_signal() {
        let out = execute_tier_signal(&serde_json::json!({
            "action_result": "fail",
            "repetition_count": 3,
            "failure_cause": "precision_miss",
        }))
        .unwrap();
        assert!(out.contains("action_result=fail"));
        assert!(out.contains("repetition_count=3"));
        assert!(out.contains("failure_cause=precision_miss"));
    }

    #[test]
    fn missing_repetition_count_defaults_to_zero() {
        let out = execute_tier_signal(&serde_json::json!({
            "action_result": "pending"
        }))
        .unwrap();
        assert!(out.contains("repetition_count=0"));
    }

    #[test]
    fn accepts_string_repetition_count() {
        let out = execute_tier_signal(&serde_json::json!({
            "action_result": "pass",
            "repetition_count": "2"
        }))
        .unwrap();
        assert!(out.contains("repetition_count=2"));
    }

    #[test]
    fn rejects_fail_without_failure_cause() {
        let err = execute_tier_signal(&serde_json::json!({
            "action_result": "fail",
            "repetition_count": 1
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("failure_cause"));
    }

    #[test]
    fn rejects_failure_cause_on_pass() {
        let err = execute_tier_signal(&serde_json::json!({
            "action_result": "pass",
            "repetition_count": 0,
            "failure_cause": "wrong_operation"
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("only allowed"));
    }

    #[test]
    fn accepts_pending_without_failure_cause() {
        let out = execute_tier_signal(&serde_json::json!({
            "action_result": "pending",
            "repetition_count": 1
        }))
        .unwrap();
        assert!(out.contains("action_result=pending"));
    }
}
