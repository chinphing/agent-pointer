use super::super::super::app_state::AppState;
use super::super::types::ToolExecResult;
use crate::dispatcher::TriggerSource;
use crate::im_ask_user::IM_ASK_USER_TIMEOUT;
use crate::models::ToolCall;
use crate::tools::ask_user::{parse_args, AskUserArgs};
use serde_json::json;
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

/// IM / Hermes: allow free-text answers that are not in the option list.
fn validate_im_selection(args: &AskUserArgs, selected: Vec<String>) -> anyhow::Result<Vec<String>> {
    if selected.is_empty() {
        anyhow::bail!("ask_user 至少需要一个回答");
    }
    if !args.multi_select && selected.len() != 1 {
        anyhow::bail!("ask_user 当前问题只允许单选");
    }
    let mut deduped = Vec::new();
    for value in selected {
        let t = value.trim();
        if t.is_empty() {
            continue;
        }
        let owned = t.to_string();
        if !deduped.contains(&owned) {
            deduped.push(owned);
        }
    }
    if deduped.is_empty() {
        anyhow::bail!("ask_user 至少需要一个回答");
    }
    Ok(deduped)
}

pub(super) async fn dispatch_ask_user(
    state: &AppState,
    tc: &ToolCall,
    args_value: serde_json::Value,
    cancel: &CancellationToken,
    trigger_source: &Option<TriggerSource>,
    conversation_id: &str,
) -> ToolExecResult {
    let args = parse_args(args_value)?;
    let is_im = matches!(trigger_source, Some(TriggerSource::Im));

    let (tx, rx) = oneshot::channel();
    state.ask_user_pending.lock().insert(tc.id.clone(), tx);

    if is_im {
        state
            .im_ask_user
            .register(conversation_id, &tc.id, args.clone());
        log::info!(
            "ask_user: IM blocking wait conversation_id={conversation_id} tool={}",
            tc.id
        );
    }

    let wait = async {
        tokio::select! {
            response = rx => response.map_err(|_| anyhow::anyhow!("ask_user 已取消")),
            _ = cancel.cancelled() => {
                Err(anyhow::anyhow!("ask_user 已取消"))
            }
        }
    };

    let selected = if is_im {
        match tokio::time::timeout(IM_ASK_USER_TIMEOUT, wait).await {
            Ok(Ok(selected)) => selected,
            Ok(Err(e)) => {
                state.ask_user_pending.lock().remove(&tc.id);
                state.im_ask_user.clear_for_desktop(conversation_id);
                return Err(e);
            }
            Err(_) => {
                state.ask_user_pending.lock().remove(&tc.id);
                state.im_ask_user.clear_for_desktop(conversation_id);
                log::warn!(
                    "ask_user: IM timed out after {}s conversation_id={conversation_id}",
                    IM_ASK_USER_TIMEOUT.as_secs()
                );
                let result = json!({
                    "selected": null,
                    "timed_out": true,
                    "note": format!(
                        "用户未在 {} 秒内回复选项。可自行合理默认，或再次询问。",
                        IM_ASK_USER_TIMEOUT.as_secs()
                    )
                });
                return Ok((result.to_string(), true, None));
            }
        }
    } else {
        match wait.await {
            Ok(selected) => selected,
            Err(e) => {
                state.ask_user_pending.lock().remove(&tc.id);
                return Err(e);
            }
        }
    };

    state.im_ask_user.clear_for_desktop(conversation_id);
    let selected = if is_im {
        validate_im_selection(&args, selected)?
    } else {
        validate_selection(&args, selected)?
    };
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
                AskUserOption {
                    label: "A".into(),
                    description: None,
                },
                AskUserOption {
                    label: "B".into(),
                    description: None,
                },
            ],
            multi_select,
        }
    }

    #[test]
    fn accepts_known_single_choice() {
        assert_eq!(
            validate_selection(&args(false), vec!["A".into()]).unwrap(),
            vec!["A"]
        );
    }

    #[test]
    fn rejects_unknown_choice() {
        assert!(validate_selection(&args(false), vec!["C".into()]).is_err());
    }
}
