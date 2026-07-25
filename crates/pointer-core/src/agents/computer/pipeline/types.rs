//! Shared types for the Advanced modular pipeline (decision → position → execute → verify).

use crate::agent_instance_scope::AgentInstanceScope;
use crate::agents::computer::ScreenCaptureResult;
use crate::llm_token_stats::ConversationLlmStats;
use serde::{Deserialize, Serialize};

/// Token usage sink for non-streaming pipeline LLM calls (Position / Verify).
pub struct PipelineLlmUsageRecorder<'a> {
    pub stats: &'a mut ConversationLlmStats,
    pub scope: &'a AgentInstanceScope,
}

/// Outcome label from the verify module (maps to tier history `verify_result`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionResult {
    Pass,
    Fail,
    Pending,
    Na,
}

impl ActionResult {
    pub fn as_history_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Pending => "pending",
            Self::Na => "n/a",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            "pending" => Some(Self::Pending),
            "n/a" | "na" => Some(Self::Na),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCause {
    WrongOperation,
    PrecisionMiss,
}

impl FailureCause {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WrongOperation => "wrong_operation",
            Self::PrecisionMiss => "precision_miss",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "wrong_operation" => Some(Self::WrongOperation),
            "precision_miss" => Some(Self::PrecisionMiss),
            _ => None,
        }
    }
}

/// Verify module JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyModuleOutput {
    pub action_result: String,
    #[serde(default)]
    pub failure_cause: Option<String>,
    #[serde(default)]
    pub step_summary: Option<String>,
    #[serde(default)]
    pub loading_detected: bool,
}

/// Conclusion stored for the next decision round.
#[derive(Debug, Clone)]
pub struct VerifyConclusion {
    pub action_result: ActionResult,
    pub failure_cause: Option<FailureCause>,
    pub step_summary: Option<String>,
}

impl VerifyConclusion {
    pub fn from_module_output(out: &VerifyModuleOutput) -> anyhow::Result<Self> {
        let action_result = ActionResult::parse(&out.action_result)
            .ok_or_else(|| anyhow::anyhow!("invalid action_result: {}", out.action_result))?;
        let failure_cause = match action_result {
            ActionResult::Fail => {
                let cause = out
                    .failure_cause
                    .as_deref()
                    .and_then(FailureCause::parse)
                    .ok_or_else(|| anyhow::anyhow!("failure_cause required on fail"))?;
                Some(cause)
            }
            _ => {
                // LLM may include failure_cause even on non-fail; ignore gracefully.
                None
            }
        };
        if matches!(action_result, ActionResult::Pass) {
            if out
                .step_summary
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .is_none()
            {
                return Err(anyhow::anyhow!("step_summary required on pass"));
            }
        }
        Ok(Self {
            action_result,
            failure_cause,
            step_summary: out.step_summary.clone(),
        })
    }

    pub fn format_for_decision_context(&self) -> String {
        match &self.action_result {
            ActionResult::Pass => format!(
                "pass{}",
                self.step_summary
                    .as_ref()
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default()
            ),
            ActionResult::Fail => format!(
                "fail ({})",
                self.failure_cause
                    .as_ref()
                    .map(FailureCause::as_str)
                    .unwrap_or("unknown")
            ),
            ActionResult::Pending => "pending".to_string(),
            ActionResult::Na => "n/a".to_string(),
        }
    }

    /// User-visible verify line on the tool card (after host post-execute).
    pub fn verify_line_for_tool_card(&self) -> String {
        match &self.action_result {
            ActionResult::Pass => format!(
                "Verify: pass{}",
                self.step_summary
                    .as_ref()
                    .map(|s| format!(" — {s}"))
                    .unwrap_or_default()
            ),
            ActionResult::Fail => format!(
                "Verify: fail ({})",
                self.failure_cause
                    .as_ref()
                    .map(FailureCause::as_str)
                    .unwrap_or("unknown")
            ),
            ActionResult::Pending => "Verify: pending".to_string(),
            ActionResult::Na => "Verify: n/a".to_string(),
        }
    }
}

/// Per-conversation pipeline cache (screenshot reuse + last verify).
#[derive(Debug, Clone, Default)]
pub struct AdvancedPipelineSession {
    pub cached_capture: Option<ScreenCaptureResult>,
    pub last_verify: Option<VerifyConclusion>,
    pub last_operation_summary: Option<String>,
    /// Sequential round number for this pipeline session (file naming / tracing).
    pub pipeline_round_seq: u32,
}

/// Which virtual position submit tool the Position LLM called.
/// Execution routing follows this — not field-shape inference alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PositionSubmitRoute {
    #[default]
    Unknown,
    /// `submit_position_index`
    Index,
    /// `submit_position_at` (x, y + reference_index anchor)
    At,
    /// `submit_position_xy`
    Xy,
    /// `submit_position_indices`
    Indices,
    /// `submit_position_positions`
    Positions,
}

/// Position module JSON output (operation-family-specific fields merged by host).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PositionModuleOutput {
    #[serde(default)]
    pub submit_route: Option<PositionSubmitRoute>,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub from_index: Option<u32>,
    #[serde(default)]
    pub to_index: Option<u32>,
    #[serde(default)]
    pub x1: Option<i32>,
    #[serde(default)]
    pub y1: Option<i32>,
    #[serde(default)]
    pub x2: Option<i32>,
    #[serde(default)]
    pub y2: Option<i32>,
    #[serde(default)]
    pub reference_index: Option<u32>,
    /// Overlay index list (`MultipleIndexOrXy` model — drag, modified_click).
    #[serde(default)]
    pub indices: Option<Vec<u32>>,
    /// Session coordinate list (`MultipleIndexOrXy` model — drag, modified_click).
    #[serde(default)]
    pub positions: Option<Vec<PositionPoint>>,
}

/// One session-normalized point in a `positions` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionPoint {
    pub x: i32,
    pub y: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_conclusion_from_pass_requires_step_summary() {
        let out = VerifyModuleOutput {
            action_result: "pass".into(),
            failure_cause: None,
            step_summary: Some("Toward goal: clicked send".into()),
            loading_detected: false,
        };
        let c = VerifyConclusion::from_module_output(&out).unwrap();
        assert_eq!(c.action_result, ActionResult::Pass);
    }

    #[test]
    fn verify_conclusion_from_fail_requires_cause() {
        let out = VerifyModuleOutput {
            action_result: "fail".into(),
            failure_cause: Some("precision_miss".into()),
            step_summary: None,
            loading_detected: false,
        };
        let c = VerifyConclusion::from_module_output(&out).unwrap();
        assert_eq!(c.failure_cause.map(|x| x.as_str()), Some("precision_miss"));
    }

    #[test]
    fn verify_line_for_tool_card_na() {
        let c = VerifyConclusion {
            action_result: ActionResult::Na,
            failure_cause: None,
            step_summary: None,
        };
        assert_eq!(c.verify_line_for_tool_card(), "Verify: n/a");
    }
}
