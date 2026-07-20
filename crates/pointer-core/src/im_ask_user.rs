//! Hermes-style IM `ask_user`: block the same turn and resolve via inbound intercept.

use crate::channel_outbound::im_base_conversation_id;
use crate::tools::ask_user::AskUserArgs;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Default wait for an IM reply (Hermes gateway clarify timeout ≈ 600s).
pub const IM_ASK_USER_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct ImAskUserPending {
    pub tool_call_id: String,
    pub desktop_conversation_id: String,
    pub args: AskUserArgs,
}

#[derive(Default)]
pub struct ImAskUserRegistry {
    /// Keyed by IM **base** conversation id (no `@s{epoch}` suffix).
    by_base_conv: Mutex<HashMap<String, ImAskUserPending>>,
}

impl ImAskUserRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &self,
        desktop_conversation_id: &str,
        tool_call_id: &str,
        args: AskUserArgs,
    ) -> String {
        let base = im_base_conversation_id(desktop_conversation_id);
        let pending = ImAskUserPending {
            tool_call_id: tool_call_id.to_string(),
            desktop_conversation_id: desktop_conversation_id.to_string(),
            args,
        };
        if let Some(prev) = self.by_base_conv.lock().unwrap().insert(base.clone(), pending) {
            log::warn!(
                "im_ask_user: replaced pending for base_conv={base} prev_tool={}",
                prev.tool_call_id
            );
        } else {
            log::info!(
                "im_ask_user: registered base_conv={base} tool_call_id={tool_call_id}"
            );
        }
        base
    }

    pub fn take(&self, base_conversation_id: &str) -> Option<ImAskUserPending> {
        self.by_base_conv
            .lock()
            .unwrap()
            .remove(base_conversation_id)
    }

    pub fn peek(&self, base_conversation_id: &str) -> Option<ImAskUserPending> {
        self.by_base_conv
            .lock()
            .unwrap()
            .get(base_conversation_id)
            .cloned()
    }

    pub fn clear_for_desktop(&self, desktop_conversation_id: &str) {
        let base = im_base_conversation_id(desktop_conversation_id);
        if self.by_base_conv.lock().unwrap().remove(&base).is_some() {
            log::info!("im_ask_user: cleared base_conv={base} (desktop end/cancel)");
        }
    }

    pub fn clear_for_base(&self, base_conversation_id: &str) -> Option<ImAskUserPending> {
        let removed = self.by_base_conv.lock().unwrap().remove(base_conversation_id);
        if removed.is_some() {
            log::info!("im_ask_user: cleared base_conv={base_conversation_id}");
        }
        removed
    }

    /// Clear every pending whose base or desktop id matches `conversation_id`.
    pub fn clear_matching(&self, conversation_id: &str) -> Vec<ImAskUserPending> {
        let base = im_base_conversation_id(conversation_id);
        let mut guard = self.by_base_conv.lock().unwrap();
        let keys: Vec<String> = guard
            .iter()
            .filter(|(k, v)| {
                k.as_str() == conversation_id
                    || k.as_str() == base
                    || v.desktop_conversation_id == conversation_id
            })
            .map(|(k, _)| k.clone())
            .collect();
        let mut out = Vec::new();
        for k in keys {
            if let Some(p) = guard.remove(&k) {
                out.push(p);
            }
        }
        out
    }
}

/// Parse an IM free-text reply into selected option labels / free-text answers.
///
/// Hermes text fallback: map `1` / `1.` → option label when possible; otherwise
/// pass the raw reply through (including free-form answers). Never reject a
/// non-empty reply — the model interprets `user_response` / `selected`.
///
/// Multi-select: comma / Chinese comma / whitespace tokens; each token is
/// mapped the same way. If any token is empty after split, treat the whole
/// string as one free-text answer.
pub fn parse_im_ask_user_reply(args: &AskUserArgs, raw: &str) -> Result<Vec<String>, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("请回复选项编号、选项原文，或直接说明你的选择。".into());
    }

    if !args.multi_select {
        return Ok(vec![resolve_one_option_or_raw(args, text)]);
    }

    let tokens: Vec<&str> = text
        .split(|c: char| c == ',' || c == '，' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return Ok(vec![text.to_string()]);
    }

    let mut selected = Vec::new();
    for token in tokens {
        let label = resolve_one_option_or_raw(args, token);
        if !selected.contains(&label) {
            selected.push(label);
        }
    }
    Ok(selected)
}

/// Map index / exact label when possible; otherwise keep the token as free text.
fn resolve_one_option_or_raw(args: &AskUserArgs, token: &str) -> String {
    let trimmed = token
        .trim()
        .trim_end_matches(|c: char| c == '.' || c == ')' || c == '、');
    if let Ok(n) = trimmed.parse::<usize>() {
        if (1..=args.options.len()).contains(&n) {
            return args.options[n - 1].label.clone();
        }
    }
    let lower = trimmed.to_lowercase();
    if let Some(opt) = args
        .options
        .iter()
        .find(|o| o.label.eq_ignore_ascii_case(trimmed) || o.label.to_lowercase() == lower)
    {
        return opt.label.clone();
    }
    token.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ask_user::AskUserOption;

    fn sample(multi: bool) -> AskUserArgs {
        AskUserArgs {
            question: "Pick".into(),
            options: vec![
                AskUserOption {
                    label: "Allow".into(),
                    description: None,
                },
                AskUserOption {
                    label: "Deny".into(),
                    description: None,
                },
            ],
            multi_select: multi,
        }
    }

    #[test]
    fn parses_index_and_label() {
        let args = sample(false);
        assert_eq!(parse_im_ask_user_reply(&args, "1").unwrap(), vec!["Allow"]);
        assert_eq!(parse_im_ask_user_reply(&args, "2.").unwrap(), vec!["Deny"]);
        assert_eq!(
            parse_im_ask_user_reply(&args, "allow").unwrap(),
            vec!["Allow"]
        );
    }

    #[test]
    fn multi_select_comma() {
        let args = sample(true);
        assert_eq!(
            parse_im_ask_user_reply(&args, "1, 2").unwrap(),
            vec!["Allow", "Deny"]
        );
    }

    #[test]
    fn free_text_passes_through_like_hermes() {
        let args = sample(false);
        assert_eq!(
            parse_im_ask_user_reply(&args, "我自己有别的方案").unwrap(),
            vec!["我自己有别的方案"]
        );
        // Out-of-range index is free text (Hermes passes raw "9" through)
        assert_eq!(parse_im_ask_user_reply(&args, "9").unwrap(), vec!["9"]);
    }

    #[test]
    fn rejects_empty() {
        let args = sample(false);
        assert!(parse_im_ask_user_reply(&args, "   ").is_err());
    }

    #[test]
    fn registry_keys_by_base() {
        let reg = ImAskUserRegistry::new();
        let base = reg.register(
            "feishu:default:feishu:dm:a:b@s3",
            "tc1",
            sample(false),
        );
        assert_eq!(base, "feishu:default:feishu:dm:a:b");
        assert!(reg.peek(&base).is_some());
        assert_eq!(reg.take(&base).unwrap().tool_call_id, "tc1");
        assert!(reg.peek(&base).is_none());
    }
}
