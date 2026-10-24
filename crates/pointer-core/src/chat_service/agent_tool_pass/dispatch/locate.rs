//! `locate_element` async dispatch — B5 visual grounding.
//!
//! Runs on the chat runtime async path (like `web_search`) because it needs the
//! conversation's [`ComputerState`] for a fresh screenshot plus the agent's
//! resolved provider for the vision calls.

use crate::models::ToolCall;
use crate::provider::OpenAIProvider;
use tokio_util::sync::CancellationToken;

use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;

pub(super) async fn dispatch_locate_element(
    state: &AppState,
    conversation_id: &str,
    provider: &OpenAIProvider,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
) -> ToolExecResult {
    let _ = tc;
    let locate_provider = super::grounding::locate_provider(state, conversation_id, provider)?;
    let payload = crate::agents::computer::locate::run_locate_element(
        state.computer_state.as_ref(),
        conversation_id,
        &locate_provider,
        &args_value,
        cancel,
    )
    .await?;

    // `run_locate_element` returns `Ok` for a completed search even when every
    // element came back `not_found` — the search ran, the target is absent.
    // Reporting that as a successful tool call makes the card read "success"
    // for a lookup that found nothing, so the outcome flag follows the payload.
    let ok = locate_payload_has_a_hit(&payload);
    let note = (!ok).then(|| "定位失败：没有找到目标".to_string());
    Ok((payload, ok, note))
}

/// `true` when at least one result in the payload has `status: "located"`.
fn locate_payload_has_a_hit(payload: &str) -> bool {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(payload) else {
        // Unparseable payloads are the caller's problem to surface, not a hit.
        return false;
    };
    parsed
        .get("results")
        .and_then(|r| r.as_array())
        .is_some_and(|rows| {
            rows.iter()
                .any(|row| row.get("status").and_then(|s| s.as_str()) == Some("located"))
        })
}

#[cfg(test)]
mod tests {
    use super::locate_payload_has_a_hit;

    #[test]
    fn a_located_result_counts_as_a_hit() {
        let payload = r#"{"results":[{"status":"located","target_desc":"Save","bbox":[1,2,3,4]}]}"#;
        assert!(locate_payload_has_a_hit(payload));
    }

    #[test]
    fn not_found_is_not_a_hit() {
        // The search ran and completed — but it found nothing, so the card must
        // not read as a successful call.
        let payload = r#"{"results":[{"status":"not_found","target_desc":"Save","summary":"..."}]}"#;
        assert!(!locate_payload_has_a_hit(payload));
    }

    #[test]
    fn one_hit_among_several_is_enough() {
        let payload = r#"{"results":[{"status":"not_found"},{"status":"located"}]}"#;
        assert!(locate_payload_has_a_hit(payload));
    }

    #[test]
    fn empty_or_malformed_payloads_are_not_hits() {
        assert!(!locate_payload_has_a_hit(r#"{"results":[]}"#));
        assert!(!locate_payload_has_a_hit(r#"{"results":null}"#));
        assert!(!locate_payload_has_a_hit("not json"));
        assert!(!locate_payload_has_a_hit(""));
    }
}
