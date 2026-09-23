//! Same-conversation idle merge push: after the lead turn ends, deliver
//! unclaimed Completed/Failed bodies of jobs the lead itself spawned, as one
//! internal `run_chat`. Jobs spawned by a sub-agent stay unclaimed for that
//! agent's `job.await` and do not start a lead turn.
//!
//! Mutex with `job.await` via `claimed`. Cancelled jobs and process exit do
//! not push. Debounces near-simultaneous finishes into one turn.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::dispatcher::hooks::{
    HookIdentity, OnRunCancelledHook, OnRunFailedHook, OnRunFinishedHook, RunCancelledContext,
    RunFailedContext, RunFinishedContext,
};
use crate::dispatcher::{DeliverTarget, RunDispatcher, TriggerMeta, TriggerRequest, TriggerSource};
use crate::models::{ChatMessage, StreamEvent};
use crate::stream_broadcast;

use super::job_supervisor::IdlePushItem;

const DEBOUNCE: Duration = Duration::from_millis(500);
const INTERNAL_LABEL: &str = "idle_job_push";

#[derive(Clone)]
struct IdleJobPush {
    dispatcher: RunDispatcher,
    gens: Arc<Mutex<HashMap<String, u64>>>,
}

impl IdleJobPush {
    fn new(dispatcher: RunDispatcher) -> Self {
        Self {
            dispatcher,
            gens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn schedule(&self, conversation_id: String) {
        if conversation_id.trim().is_empty() {
            return;
        }
        let gen = {
            let mut gens = self.gens.lock();
            let slot = gens.entry(conversation_id.clone()).or_insert(0);
            *slot = slot.wrapping_add(1);
            *slot
        };
        let this = self.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            log::warn!(
                "idle_job_push: no tokio runtime; skip schedule conversation_id={conversation_id}"
            );
            return;
        };
        handle.spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;
            if this.gens.lock().get(&conversation_id).copied() != Some(gen) {
                return;
            }
            this.try_flush(&conversation_id).await;
        });
    }

    fn lead_busy(&self, conversation_id: &str) -> bool {
        let state = self.dispatcher.app_state();
        if state.cancels.lock().contains_key(conversation_id) {
            return true;
        }
        if self
            .dispatcher
            .queue()
            .session_has_activity(conversation_id)
        {
            return true;
        }
        match state
            .session_index
            .runs_list_non_terminal_for_conversation(conversation_id, 8)
        {
            Ok(rows) => !rows.is_empty(),
            Err(err) => {
                log::warn!(
                    "idle_job_push: list non-terminal runs failed conversation_id={conversation_id}: {err:#}"
                );
                true
            }
        }
    }

    async fn try_flush(&self, conversation_id: &str) {
        if self.lead_busy(conversation_id) {
            log::info!("idle_job_push: defer; lead busy conversation_id={conversation_id}");
            // Job finish may race lead teardown. Reschedule so a deferred claim
            // still flushes once the lead is idle (do not rely only on the next
            // job finish or on_run_finished).
            self.schedule(conversation_id.to_string());
            return;
        }
        let state = self.dispatcher.app_state();
        let items = state.jobs.claim_pushable(conversation_id);
        if items.is_empty() {
            return;
        }
        let job_ids: Vec<String> = items.iter().map(|i| i.job_id.clone()).collect();
        let bubble = build_idle_push_bubble_text(&items);
        log::info!(
            "idle_job_push: flushing conversation_id={conversation_id} count={} bubble={bubble}",
            items.len()
        );
        let mut user_msg = ChatMessage::user_text(build_idle_push_user_text(&items));
        user_msg.ui_bindings = Some(crate::models::MessageUiBindings::idle_job_push_bubble(
            bubble,
        ));
        let user_msg_id = user_msg.id.clone();
        let user_msg_content = user_msg.content.clone();
        let user_msg_ui = user_msg.ui_bindings.clone();
        // Delta only. `prepare_lead_history` appends this row and reloads the
        // lead working set. Loading the full transcript here retains every
        // soft-excluded payload (10k+ rows) for the rest of the process.
        let messages = vec![user_msg];
        log::info!(
            "idle_job_push: dispatch delta conversation_id={conversation_id} dispatch_messages={}",
            messages.len()
        );
        stream_broadcast::broadcast_stream(&StreamEvent::InjectedUserMessage {
            conversation_id: conversation_id.to_string(),
            message_id: user_msg_id,
            content: user_msg_content,
            attachments: None,
            ui_bindings: user_msg_ui,
        });
        let meta = state
            .session_index
            .load_meta(conversation_id)
            .ok()
            .flatten();
        let req = TriggerRequest {
            run_id: None,
            idempotency_key: None,
            conversation_id: Some(conversation_id.to_string()),
            trigger_source: TriggerSource::Internal,
            trigger_meta: TriggerMeta {
                internal_label: Some(INTERNAL_LABEL.into()),
                ..TriggerMeta::empty()
            },
            lane: None,
            messages,
            enabled_skill_ids: Vec::new(),
            agent_skill_overrides: HashMap::new(),
            agent_mode: meta.as_ref().map(|m| m.agent_mode.clone()),
            lead_agent_id: meta.as_ref().map(|m| m.lead_agent_id.clone()),
            performance_mode: meta.as_ref().and_then(|m| m.performance_mode.clone()),
            tool_rounds_used_single_start: 0,
            tool_rounds_used_supervisor_start: 0,
            workspace_root: meta
                .as_ref()
                .map(|m| m.workspace_root.clone())
                .unwrap_or_default(),
            workspace_inherit_disabled: meta.as_ref().map(|m| m.workspace_inherit_disabled),
            deliver: DeliverTarget::None,
            // Same as cron/webhook: reuse browser/local session (or cached LLM keys)
            // so standalone server idle push is not gated as "请先登录".
            web_session_auth: state.automation_execution_auth(),
        };
        if let Err(err) = self.dispatcher.dispatch(req).await {
            log::error!(
                "idle_job_push: dispatch failed conversation_id={conversation_id}: {err:#}"
            );
            state.jobs.unclaim(&job_ids);
        }
    }
}

const BUBBLE_TITLE_CHARS: usize = 20;
const BUBBLE_TITLE_MAX: usize = 2;

fn idle_push_item_title(item: &IdlePushItem) -> String {
    let raw = item
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let text = match raw {
        Some(s) => s,
        None => match item.kind {
            "terminal" => "终端",
            _ => "子任务",
        },
    };
    crate::text_util::truncate_chars_fit(text, BUBBLE_TITLE_CHARS)
}

fn join_idle_push_titles(titles: &[String]) -> String {
    match titles.len() {
        0 => String::new(),
        1 => titles[0].clone(),
        n if n <= BUBBLE_TITLE_MAX => titles.join("、"),
        n => format!("{}、{} 等 {n} 个", titles[0], titles[1]),
    }
}

/// Short user-facing bubble. Full bodies stay in `content` for the lead.
pub(crate) fn build_idle_push_bubble_text(items: &[IdlePushItem]) -> String {
    if items.is_empty() {
        return "后台任务已完成。".into();
    }
    let all_failed = items.iter().all(|item| item.status == "failed");
    let prefix = if all_failed {
        "后台任务失败"
    } else {
        "后台任务已完成"
    };
    let titles: Vec<String> = items.iter().map(idle_push_item_title).collect();
    let shown = join_idle_push_titles(&titles);
    if shown.is_empty() {
        return format!("{prefix}。");
    }
    format!("{prefix}：{shown}")
}

pub(crate) fn build_idle_push_user_text(items: &[IdlePushItem]) -> String {
    let n = items.len();
    let mut out = String::from("后台任务已完成。\n\n");
    out.push_str("These finished background results are in this turn.\n");
    out.push_str("They are already claimed — do not job.await these ids again.\n");
    out.push_str("Other background jobs in this conversation may still be running.\n");
    out.push_str("Only summarize these finished jobs; do not claim all work is done\n");
    out.push_str("unless nothing else is still running.\n");
    out.push_str("Summarize for the user. Do not paste worker thoughts.\n");
    if n > 1 {
        out.push_str(&format!("\n{n} jobs:\n"));
    }
    for item in items {
        let title = item
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(item.job_id.as_str());
        out.push_str(&format!(
            "\n### {title}\nkind: {}\nstatus: {}\njobId: {}\n",
            item.kind, item.status, item.job_id
        ));
        if let Some(agent_id) = item.agent_id.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("agentId: {agent_id}\n"));
        }
        if let Some(instance_id) = item
            .agent_instance_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            out.push_str(&format!("agentInstanceId: {instance_id}\n"));
            out.push_str(
                "To continue this same worker, call run_subagent with followupInstanceId set to that agentInstanceId.\n",
            );
        }
        if let Some(err) = item.error.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("error: {err}\n"));
        }
        if let Some(content) = item.content.as_deref().filter(|s| !s.is_empty()) {
            out.push_str("\n");
            out.push_str(content);
            out.push('\n');
        }
    }
    out
}

/// Wire job-finish + lead-end hooks. Safe to call from host setup (no spawn).
pub fn install(dispatcher: &RunDispatcher) {
    let push = IdleJobPush::new(dispatcher.clone());
    let hook = Arc::new(IdleJobPushHook { push: push.clone() });
    dispatcher.hooks().register_on_run_finished(hook.clone());
    dispatcher.hooks().register_on_run_failed(hook.clone());
    dispatcher.hooks().register_on_run_cancelled(hook);
    let for_jobs = push.clone();
    dispatcher
        .app_state()
        .jobs
        .set_on_pushable(Arc::new(move |conversation_id| {
            for_jobs.schedule(conversation_id);
        }));
    log::info!("idle_job_push: installed");
}

struct IdleJobPushHook {
    push: IdleJobPush,
}

impl HookIdentity for IdleJobPushHook {
    fn override_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("builtin.idle_job_push")
    }
    fn sort_key(&self) -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("_40_idle_job_push")
    }
}

#[async_trait]
impl OnRunFinishedHook for IdleJobPushHook {
    async fn execute(&self, ctx: &RunFinishedContext) -> anyhow::Result<()> {
        self.push.schedule(ctx.conversation_id.clone());
        Ok(())
    }
}

#[async_trait]
impl OnRunFailedHook for IdleJobPushHook {
    async fn execute(&self, ctx: &RunFailedContext) -> anyhow::Result<()> {
        self.push.schedule(ctx.conversation_id.clone());
        Ok(())
    }
}

#[async_trait]
impl OnRunCancelledHook for IdleJobPushHook {
    async fn execute(&self, ctx: &RunCancelledContext) -> anyhow::Result<()> {
        self.push.schedule(ctx.conversation_id.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_text_starts_with_short_chinese_and_includes_bodies() {
        let text = build_idle_push_user_text(&[IdlePushItem {
            job_id: "job_a".into(),
            status: "completed",
            kind: "subagent",
            title: Some("搜索登录".into()),
            agent_id: Some("explore".into()),
            content: Some("found login.rs".into()),
            error: None,
            agent_instance_id: Some("inst-1".into()),
        }]);
        assert!(text.starts_with("后台任务已完成。"));
        assert!(text.contains("搜索登录"));
        assert!(text.contains("found login.rs"));
        assert!(text.contains("already claimed"));
        assert!(text.contains("agentInstanceId: inst-1"));
        assert!(text.contains("followupInstanceId"));
        assert!(text.contains("may still be running"));
        assert!(!text.to_lowercase().contains("thoughts:"));
        assert_eq!(
            build_idle_push_bubble_text(&[IdlePushItem {
                job_id: "job_a".into(),
                status: "completed",
                kind: "subagent",
                title: Some("搜索登录".into()),
                agent_id: Some("explore".into()),
                content: Some("found login.rs".into()),
                error: None,
                agent_instance_id: None,
            }]),
            "后台任务已完成：搜索登录"
        );
    }

    #[test]
    fn user_text_merges_multiple_jobs() {
        let text = build_idle_push_user_text(&[
            IdlePushItem {
                job_id: "j1".into(),
                status: "completed",
                kind: "subagent",
                title: Some("A".into()),
                agent_id: None,
                content: Some("one".into()),
                error: None,
                agent_instance_id: None,
            },
            IdlePushItem {
                job_id: "j2".into(),
                status: "failed",
                kind: "terminal",
                title: Some("测测试".into()),
                agent_id: None,
                content: None,
                error: Some("exit 1".into()),
                agent_instance_id: None,
            },
        ]);
        assert!(text.contains("2 jobs:"));
        assert!(text.contains("### A"));
        assert!(text.contains("exit 1"));
        assert_eq!(
            build_idle_push_bubble_text(&[
                IdlePushItem {
                    job_id: "j1".into(),
                    status: "completed",
                    kind: "subagent",
                    title: Some("A".into()),
                    agent_id: None,
                    content: Some("one".into()),
                    error: None,
                    agent_instance_id: None,
                },
                IdlePushItem {
                    job_id: "j2".into(),
                    status: "failed",
                    kind: "terminal",
                    title: Some("测测试".into()),
                    agent_id: None,
                    content: None,
                    error: Some("exit 1".into()),
                    agent_instance_id: None,
                },
            ]),
            "后台任务已完成：A、测测试"
        );
    }

    fn push_item(status: &'static str, kind: &'static str, title: Option<&str>) -> IdlePushItem {
        IdlePushItem {
            job_id: "j".into(),
            status,
            kind,
            title: title.map(str::to_string),
            agent_id: None,
            content: None,
            error: None,
            agent_instance_id: None,
        }
    }

    #[test]
    fn bubble_text_uses_title_and_failure_prefix() {
        assert_eq!(
            build_idle_push_bubble_text(&[push_item("failed", "subagent", Some("跑测试"))]),
            "后台任务失败：跑测试"
        );
        assert_eq!(
            build_idle_push_bubble_text(&[push_item("completed", "terminal", None)]),
            "后台任务已完成：终端"
        );
        let many = vec![
            push_item("completed", "subagent", Some("线 1")),
            push_item("completed", "subagent", Some("线 2 首都圈")),
            push_item("completed", "subagent", Some("线 3")),
        ];
        assert_eq!(
            build_idle_push_bubble_text(&many),
            "后台任务已完成：线 1、线 2 首都圈 等 3 个"
        );
        let long = "abcdefghijklmnopqrstuvwxyz";
        let bubble = build_idle_push_bubble_text(&[push_item("completed", "subagent", Some(long))]);
        assert!(bubble.starts_with("后台任务已完成："));
        assert!(bubble.contains('…'));
        assert!(bubble.chars().count() < 20 + "后台任务已完成：".chars().count() + 2);
    }
}
