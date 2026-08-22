use super::deps_hint::MEDIA_DEPS_MARKER;
use crate::models::{ChatMessage, MediaAttachment, Role};

const MAX_RETRY_USER_TURNS: usize = 3;

const ATTACHMENT_RETRY_PHRASES: &[&str] = &[
    "重试上一条附件",
    "重试附件",
    "重新处理附件",
    "retry last attachment",
    "retry the attachment",
    "retry attachment",
];

const VIDEO_RETRY_PHRASES: &[&str] = &[
    "重试上一条视频",
    "重试视频",
    "retry last video",
    "retry the video",
    "retry video",
];

const MEDIA_BLOCK_PREFIXES: &[&str] = &[
    "[Attachment:",
    "[Video:",
    "[Image:",
    "[PDF:",
    "[Audio:",
    "[File:",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaRetryScope {
    /// User asked to retry attachments (docx, zip, pdf, etc.).
    AllAttachments,
    /// User asked to retry video only.
    VideoOnly,
    /// ffmpeg became available; retry videos that failed with pointer-media-deps.
    AutoFfmpegVideo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaRetryPlan {
    pub scope: MediaRetryScope,
    /// Index of the latest real user turn (retry phrase detection).
    pub latest_user_idx: usize,
}

/// `storageRelPath` present and no successful `derivedText`.
pub fn attachment_retryable(att: &MediaAttachment) -> bool {
    att.storage_rel_path
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        && att
            .derived_text
            .as_ref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true)
}

pub fn content_has_media_block_for_file(content: &str, file_name: &str) -> bool {
    content
        .split("\n\n")
        .any(|part| media_block_first_line_matches_file(part, file_name))
}

fn media_block_first_line_matches_file(part: &str, file_name: &str) -> bool {
    let first = part.lines().next().unwrap_or("").trim();
    if !first.starts_with('[') {
        return false;
    }
    MEDIA_BLOCK_PREFIXES.iter().any(|p| first.starts_with(p)) && first.contains(file_name)
}

/// Replace or drop the media injection block for `file_name`. When `new_block` is `None`, remove it.
pub fn replace_media_injection(content: &str, file_name: &str, new_block: Option<&str>) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut replaced = false;
    for part in content.split("\n\n") {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if media_block_first_line_matches_file(trimmed, file_name) {
            if let Some(block) = new_block {
                kept.push(block.to_string());
                replaced = true;
            }
            continue;
        }
        kept.push(trimmed.to_string());
    }
    if !replaced {
        if let Some(block) = new_block {
            if kept.is_empty() {
                return block.to_string();
            }
            kept.push(block.to_string());
        }
    }
    kept.join("\n\n")
}

fn content_matches_phrases(content: &str, phrases: &[&str]) -> bool {
    let lower = content.to_ascii_lowercase();
    phrases
        .iter()
        .any(|p| lower.contains(&p.to_ascii_lowercase()))
}

fn last_user_message_index(history: &[ChatMessage]) -> Option<usize> {
    history.iter().rposition(|m| matches!(m.role, Role::User))
}

fn user_message_indices_before(
    history: &[ChatMessage],
    before_idx: usize,
    max_turns: usize,
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut turns = 0usize;
    for i in (0..before_idx).rev() {
        if matches!(history[i].role, Role::User) {
            out.push(i);
            turns += 1;
            if turns >= max_turns {
                break;
            }
        }
    }
    out
}

fn all_user_message_indices(history: &[ChatMessage]) -> Vec<usize> {
    history
        .iter()
        .enumerate()
        .filter_map(|(i, m)| matches!(m.role, Role::User).then_some(i))
        .collect()
}

pub fn detect_media_retry_plan(history: &[ChatMessage]) -> Option<MediaRetryPlan> {
    let latest_user_idx = last_user_message_index(history)?;
    let content = history[latest_user_idx].content.as_str();
    if content_matches_phrases(content, VIDEO_RETRY_PHRASES) {
        return Some(MediaRetryPlan {
            scope: MediaRetryScope::VideoOnly,
            latest_user_idx,
        });
    }
    if content_matches_phrases(content, ATTACHMENT_RETRY_PHRASES)
        || content_matches_phrases(content, &["重试上一条", "retry last"])
    {
        return Some(MediaRetryPlan {
            scope: MediaRetryScope::AllAttachments,
            latest_user_idx,
        });
    }
    None
}

pub fn auto_ffmpeg_video_retry_plan(history: &[ChatMessage]) -> Option<MediaRetryPlan> {
    if !crate::media::ffmpeg::ffmpeg_available() {
        return None;
    }
    let latest_user_idx = last_user_message_index(history)?;
    Some(MediaRetryPlan {
        scope: MediaRetryScope::AutoFfmpegVideo,
        latest_user_idx,
    })
}

pub fn message_indices_for_retry(history: &[ChatMessage], plan: MediaRetryPlan) -> Vec<usize> {
    match plan.scope {
        MediaRetryScope::AllAttachments | MediaRetryScope::VideoOnly => {
            user_message_indices_before(history, plan.latest_user_idx, MAX_RETRY_USER_TURNS)
        }
        MediaRetryScope::AutoFfmpegVideo => all_user_message_indices(history),
    }
}

pub fn should_retry_attachment(
    scope: MediaRetryScope,
    msg_content: &str,
    att: &MediaAttachment,
) -> bool {
    if !attachment_retryable(att) {
        return false;
    }
    match scope {
        MediaRetryScope::AllAttachments => true,
        MediaRetryScope::VideoOnly => is_video_attachment(att),
        MediaRetryScope::AutoFfmpegVideo => {
            is_video_attachment(att) && msg_content.contains(MEDIA_DEPS_MARKER)
        }
    }
}

fn is_video_attachment(att: &MediaAttachment) -> bool {
    att.kind == "video"
        || att
            .mime_type
            .trim()
            .to_ascii_lowercase()
            .starts_with("video/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_media_injection_swaps_block() {
        let content = "请看一下\n\n[Attachment: a.docx] 无法直接解析\n\n谢谢";
        let new_block = "[Attachment: a.docx]\n已解析内容";
        let out = replace_media_injection(content, "a.docx", Some(new_block));
        assert!(out.contains("请看一下"));
        assert!(out.contains("已解析内容"));
        assert!(!out.contains("无法直接解析"));
        assert!(out.contains("谢谢"));
    }

    #[test]
    fn detect_video_retry_phrase() {
        let history = vec![ChatMessage {
            id: "1".into(),
            role: Role::User,
            content: "重试上一条视频".into(),
            status: "done".into(),
            created_at: 0,
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            error_message: None,
            reasoning: None,
            thoughts: None,
            headline: None,
            raw_content: None,
            tool_raw_output: None,
            agent_id: None,
            agent_instance_id: None,
            agent_name: None,
            agent_trace: None,
            images_base64: None,
            image_slot_labels: None,
            computer_round_screen_rel_path: None,
            ui_bindings: None,
            context_state: None,
            attachments: None,
            anchor_message_id: None,
            trace_id: None,
            task_id: None,
            spawn_depth: None,
        }];
        let plan = detect_media_retry_plan(&history).expect("plan");
        assert_eq!(plan.scope, MediaRetryScope::VideoOnly);
    }

    #[test]
    fn attachment_retryable_requires_storage_without_derived_text() {
        let mut att = MediaAttachment {
            id: "a".into(),
            kind: "file".into(),
            mime_type: "application/octet-stream".into(),
            file_name: "x.zip".into(),
            size_bytes: 0,
            storage_rel_path: Some("c/a.zip".into()),
            content_base64: None,
            derived_text: None,
            local_abs_path: None,
            remote_url: None,
            oss_object_key: None,
        };
        assert!(attachment_retryable(&att));
        att.derived_text = Some("ok".into());
        assert!(!attachment_retryable(&att));
    }
}
