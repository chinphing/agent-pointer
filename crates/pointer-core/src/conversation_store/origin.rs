//! Message provenance: who authored a user row (`messages.origin`).
//!
//! This answers a **different** question than `messages.is_system_generated`.
//! That column means "is this row a turn anchor" (paging / nav / milestones).
//! A background-job completion notice is a turn anchor *and* host-generated —
//! one boolean cannot express both, which is why provenance gets its own column.
//!
//! Rules are structural first: the write site stamps `uiBindings.hostKind`
//! (see [`crate::dispatcher`]) and that marker wins. Content prefixes are a
//! fallback for legacy rows only. A row whose provenance cannot be proven
//! returns `None` and the store writes `NULL` — never guess `human`.

use crate::models::{is_scoped_sub_message, ChatMessage, Role};

/// `uiBindings.hostKind` values. The write site knows who authored the row, so
/// it stamps this instead of leaving the store to guess from content.
pub const HOST_KIND_USER_INPUT: &str = "user_input";
pub const HOST_KIND_IM_INBOUND: &str = "im_inbound";
pub const HOST_KIND_WEBHOOK_INBOUND: &str = "webhook_inbound";
pub const HOST_KIND_CRON: &str = "cron";
pub const HOST_KIND_IDLE_JOB_PUSH: &str = "idle_job_push";
pub const HOST_KIND_WORKER_FOLLOWUP: &str = "worker_followup";
pub const HOST_KIND_INTERNAL: &str = "internal";

/// Value stored in `messages.origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageOrigin {
    /// Typed by a human in the desktop / web composer.
    Human,
    /// IM inbound (Feishu / DingTalk / WeCom / Weixin). A human wrote it; only
    /// the channel differs, so it counts as human.
    Im,
    /// Inbound webhook / CI event — machine-triggered.
    Webhook,
    /// Scheduled cron job prompt injected by the scheduler.
    HostCron,
    /// Idle background-job completion push (`idle_job_push`).
    HostJobPush,
    /// Worker resume instruction injected by `worker_followup`.
    HostWorker,
    /// Other internal trigger without a more specific label.
    HostInternal,
    /// Delegated sub-agent transcript row (never a lead user turn).
    AgentScoped,
    /// Screenshot inject / compression summary / trim placeholder / retry glue.
    Synthetic,
}

impl MessageOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Im => "im",
            Self::Webhook => "webhook",
            Self::HostCron => "host_cron",
            Self::HostJobPush => "host_job_push",
            Self::HostWorker => "host_worker",
            Self::HostInternal => "host_internal",
            Self::AgentScoped => "agent_scoped",
            Self::Synthetic => "synthetic",
        }
    }

    /// Whether a human authored the row (the only values a "user message count"
    /// should include).
    pub fn is_human(self) -> bool {
        matches!(self, Self::Human | Self::Im)
    }

    /// Map a `uiBindings.hostKind` marker to an origin. Unknown kinds return
    /// `None` on purpose: a marker we do not understand must not degrade into
    /// "human" (that is exactly the bug this column exists to fix).
    pub fn from_host_kind(kind: &str) -> Option<Self> {
        let kind = kind.trim();
        if kind == HOST_KIND_USER_INPUT {
            return Some(Self::Human);
        }
        if kind == HOST_KIND_IM_INBOUND {
            return Some(Self::Im);
        }
        if kind == HOST_KIND_WEBHOOK_INBOUND {
            return Some(Self::Webhook);
        }
        if kind == HOST_KIND_CRON {
            return Some(Self::HostCron);
        }
        if kind == HOST_KIND_IDLE_JOB_PUSH {
            return Some(Self::HostJobPush);
        }
        if kind == HOST_KIND_WORKER_FOLLOWUP {
            return Some(Self::HostWorker);
        }
        if kind == HOST_KIND_INTERNAL {
            return Some(Self::HostInternal);
        }
        None
    }
}

/// Classify one message row for `messages.origin`.
///
/// `None` = provenance unknown → the store writes `NULL`. Non-user rows are
/// always `None` (only user rows carry provenance).
pub fn classify_origin(msg: &ChatMessage) -> Option<MessageOrigin> {
    if !matches!(msg.role, Role::User) {
        return None;
    }
    // A delegated sub-agent transcript row is never a lead user turn, whatever
    // marker it carries.
    if is_scoped_sub_message(msg) {
        return Some(MessageOrigin::AgentScoped);
    }
    // Authoritative: the write site stamped provenance before dispatch.
    if let Some(kind) = msg
        .ui_bindings
        .as_ref()
        .and_then(|b| b.host_kind.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Some(origin) = MessageOrigin::from_host_kind(kind) {
            return Some(origin);
        }
        log::warn!(
            "origin: unknown host_kind '{kind}' message_id={} — leaving origin NULL",
            msg.id
        );
        return None;
    }
    // Legacy / unstamped rows: structural + content fallbacks only.
    if msg.id.starts_with("fmt_retry_") {
        return Some(MessageOrigin::Synthetic);
    }
    if crate::task_board::history_trim::is_injected_or_synthetic_user_content(&msg.content)
        || crate::message_context::is_synthetic_user_content(&msg.content)
    {
        return Some(MessageOrigin::Synthetic);
    }
    if msg
        .content
        .trim_start()
        .starts_with(crate::scheduler::CRON_USER_HINT)
    {
        return Some(MessageOrigin::HostCron);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MessageUiBindings, Role};

    fn user(content: &str) -> ChatMessage {
        ChatMessage::user_text(content)
    }

    fn stamped(content: &str, kind: &str) -> ChatMessage {
        let mut msg = ChatMessage::user_text(content);
        msg.ui_bindings = Some(MessageUiBindings {
            host_kind: Some(kind.into()),
            ..Default::default()
        });
        msg
    }

    fn scoped(content: &str, anchor: &str) -> ChatMessage {
        let mut msg = ChatMessage::user_text(content);
        msg.anchor_message_id = Some(anchor.into());
        msg
    }

    #[test]
    fn marker_wins_over_content() {
        // Same idle-push content, different stamps → different origins. This is
        // the case a content prefix table cannot decide.
        let push = stamped(
            "后台任务已完成：搜索登录\n\nThese finished background results are in this turn.\n",
            HOST_KIND_IDLE_JOB_PUSH,
        );
        assert_eq!(classify_origin(&push), Some(MessageOrigin::HostJobPush));
        assert!(!MessageOrigin::HostJobPush.is_human());

        let human = stamped("后台任务已完成：搜索登录", HOST_KIND_USER_INPUT);
        assert_eq!(classify_origin(&human), Some(MessageOrigin::Human));
        assert!(MessageOrigin::Human.is_human());
    }

    #[test]
    fn every_marker_maps_to_its_origin() {
        let cases = [
            (HOST_KIND_USER_INPUT, MessageOrigin::Human),
            (HOST_KIND_IM_INBOUND, MessageOrigin::Im),
            (HOST_KIND_WEBHOOK_INBOUND, MessageOrigin::Webhook),
            (HOST_KIND_CRON, MessageOrigin::HostCron),
            (HOST_KIND_IDLE_JOB_PUSH, MessageOrigin::HostJobPush),
            (HOST_KIND_WORKER_FOLLOWUP, MessageOrigin::HostWorker),
            (HOST_KIND_INTERNAL, MessageOrigin::HostInternal),
        ];
        for (kind, expected) in cases {
            assert_eq!(
                classify_origin(&stamped("text", kind)),
                Some(expected),
                "host_kind={kind}"
            );
            assert_eq!(MessageOrigin::from_host_kind(kind), Some(expected));
        }
    }

    #[test]
    fn unknown_marker_stays_null_not_human() {
        assert_eq!(
            classify_origin(&stamped("some future injection", "host_curator")),
            None
        );
    }

    #[test]
    fn scoped_rows_are_agent_scoped_even_with_a_marker() {
        let mut msg = scoped("sub task", "anchor-1");
        msg.ui_bindings = Some(MessageUiBindings {
            host_kind: Some(HOST_KIND_USER_INPUT.into()),
            ..Default::default()
        });
        assert_eq!(classify_origin(&msg), Some(MessageOrigin::AgentScoped));
    }

    #[test]
    fn legacy_fallbacks_without_a_marker() {
        let mut retry = user("retry glue");
        retry.id = "fmt_retry_abc".into();
        assert_eq!(classify_origin(&retry), Some(MessageOrigin::Synthetic));

        assert_eq!(
            classify_origin(&user("[CUR_SCREEN] shot")),
            Some(MessageOrigin::Synthetic)
        );
        assert_eq!(
            classify_origin(&user("【环境反馈】上游服务暂时不可用（x），正在重试。")),
            Some(MessageOrigin::Synthetic)
        );
        let cron = format!("{}do the thing", crate::scheduler::CRON_USER_HINT);
        assert_eq!(
            classify_origin(&user(&cron)),
            Some(MessageOrigin::HostCron)
        );
        // No marker, no known template → unknown, NOT human.
        assert_eq!(classify_origin(&user("帮我报销")), None);
    }

    #[test]
    fn non_user_rows_have_no_origin() {
        let mut msg = user("answer");
        msg.role = Role::Assistant;
        assert_eq!(classify_origin(&msg), None);
        let mut tool = user("ok");
        tool.role = Role::Tool;
        assert_eq!(classify_origin(&tool), None);
    }

    #[test]
    fn anchor_flag_is_derived_from_origin() {
        use crate::conversation_store::persist::is_system_generated_user_message;

        let cases = [
            (HOST_KIND_USER_INPUT, MessageOrigin::Human),
            (HOST_KIND_IM_INBOUND, MessageOrigin::Im),
            (HOST_KIND_WEBHOOK_INBOUND, MessageOrigin::Webhook),
            (HOST_KIND_CRON, MessageOrigin::HostCron),
            (HOST_KIND_IDLE_JOB_PUSH, MessageOrigin::HostJobPush),
            (HOST_KIND_WORKER_FOLLOWUP, MessageOrigin::HostWorker),
            (HOST_KIND_INTERNAL, MessageOrigin::HostInternal),
        ];
        for (kind, origin) in cases {
            let msg = stamped("text", kind);
            assert_eq!(classify_origin(&msg), Some(origin), "host_kind={kind}");
            assert_eq!(
                is_system_generated_user_message(&msg),
                !origin.is_human(),
                "anchor flag must mirror origin for host_kind={kind}"
            );
        }
        // Unknown provenance keeps the legacy anchor behaviour (flag 0).
        assert!(!is_system_generated_user_message(&user("帮我报销")));
        assert!(is_system_generated_user_message(&user("[CUR_SCREEN] x")));
        // Non-user rows are never flagged.
        let mut assistant = user("answer");
        assistant.role = Role::Assistant;
        assert!(!is_system_generated_user_message(&assistant));
    }

    #[test]
    fn origin_strings_are_stable() {
        // Persisted values — changing one is a data migration.
        assert_eq!(MessageOrigin::Human.as_str(), "human");
        assert_eq!(MessageOrigin::Im.as_str(), "im");
        assert_eq!(MessageOrigin::Webhook.as_str(), "webhook");
        assert_eq!(MessageOrigin::HostCron.as_str(), "host_cron");
        assert_eq!(MessageOrigin::HostJobPush.as_str(), "host_job_push");
        assert_eq!(MessageOrigin::HostWorker.as_str(), "host_worker");
        assert_eq!(MessageOrigin::HostInternal.as_str(), "host_internal");
        assert_eq!(MessageOrigin::AgentScoped.as_str(), "agent_scoped");
        assert_eq!(MessageOrigin::Synthetic.as_str(), "synthetic");
    }
}
