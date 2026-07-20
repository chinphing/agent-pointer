use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;
use crate::dispatcher::TriggerSource;
use crate::models::ToolCall;
use crate::tools::ask_user::{parse_args, AskUserArgs};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn validate_selection(args: &AskUserArgs, selected: Vec<String>) -> anyhow::Result<Vec<String>> {
    if selected.is_empty() {
        anyhow::bail!("ask_user 至少需要选择一个选项");
    }
    if !args.multi_select && selected.len() != 1 {
        anyhow::bail!("ask_user 当前问题只允许单选");
    }
    let labels: std::collections::HashSet<&str> =
        args.options.iter().map(|option| option.label.as_str()).collect();
    if selected.iter().any(|value| !labels.contains(value.as_str())) {
        anyhow::bail!("ask_user 提交了无效选项");
    }
    let mut deduped = Vec::new();
    for value in selected {
        if !deduped.contains(&value) {
            deduped.push(value);
        }
    }
    Ok(deduped)
}

pub(super) async fn dispatch_ask_user(
    state: &AppState,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    trigger_source: &Option<TriggerSource>,
    ask_user_deferred: &AtomicBool,
) -> ToolExecResult {
    let args = parse_args(args_value)?;

    // IM channels: non-blocking Hermes-style — signal the tool pass to end
    // the turn after this round. The LLM's already-emitted text before this
    // tool call contains the options; the IM dispatch will deliver it.
    // The next user message from the IM channel starts a fresh turn.
    if matches!(trigger_source, Some(TriggerSource::Im)) {
        ask_user_deferred.store(true, Ordering::Relaxed);
        let labels: Vec<&str> = args.options.iter().map(|o| o.label.as_str()).collect();
        let result = json!({
            "selected": null,
            "note": format!("选项已展示给用户。当前对话轮次结束，等待用户回复。选项：{:?}", labels)
        });
        return Ok((result.to_string(), true, None));
    }

    // Desktop / web: interactive blocking pattern with AskUserOptions UI.
    let (tx, rx) = oneshot::channel();
    state.ask_user_pending.lock().insert(tc.id.clone(), tx);

    let selected = tokio::select! {
        response = rx => response.map_err(|_| anyhow::anyhow!("ask_user 已取消"))?,
        _ = cancel.cancelled() => {
            state.ask_user_pending.lock().remove(&tc.id);
            anyhow::bail!("ask_user 已取消");
        }
    };
    let selected = validate_selection(&args, selected)?;
    Ok((json!({ "selected": selected }).to_string(), true, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ask_user::{AskUserArgs, AskUserOption};

    fn args(multi_select: bool) -> AskUserArgs {
        AskUserArgs {
            question: "Choose".into(),
            options: vec![
                AskUserOption { label: "A".into(), description: None },
                AskUserOption { label: "B".into(), description: None },
            ],
            multi_select,
        }
    }

    #[test]
    fn accepts_known_single_choice() {
        assert_eq!(validate_selection(&args(false), vec!["A".into()]).unwrap(), vec!["A"]);
    }

    #[test]
    fn rejects_unknown_choice() {
        assert!(validate_selection(&args(false), vec!["C".into()]).is_err());
    }
}
