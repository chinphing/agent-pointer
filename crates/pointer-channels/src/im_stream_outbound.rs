//! Mirror agent stream events to IM customers during a channel dispatch run.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use pointer_core::models::StreamEvent;

use crate::config::ImOutboundConfig;
use crate::outbound_reply::split_reply_media;
use crate::outbound_resolve::resolve_outbound_media;
use crate::traits::{ChannelPlugin, OutboundContext};

const TOOL_SUMMARY_MAX_CHARS: usize = 200;

pub struct ImStreamOutbound<'a> {
    plugin: &'a ChannelPlugin,
    outbound: OutboundContext,
    cfg: ImOutboundConfig,
    conv_id: String,
    sent_message_ids: HashSet<String>,
    sent_media: HashSet<String>,
    notified_tool_keys: HashSet<String>,
    tool_display_labels: HashMap<String, String>,
    tool_started_notified: HashSet<String>,
    last_sent_visible: String,
}

impl<'a> ImStreamOutbound<'a> {
    pub fn new(
        plugin: &'a ChannelPlugin,
        outbound: OutboundContext,
        cfg: ImOutboundConfig,
        conv_id: String,
    ) -> Self {
        Self {
            plugin,
            outbound,
            cfg,
            conv_id,
            sent_message_ids: HashSet::new(),
            sent_media: HashSet::new(),
            notified_tool_keys: HashSet::new(),
            tool_display_labels: HashMap::new(),
            tool_started_notified: HashSet::new(),
            last_sent_visible: String::new(),
        }
    }

    pub async fn on_event(&mut self, ev: &StreamEvent) -> Result<()> {
        match ev {
            StreamEvent::MessageEnd {
                message_id,
                content,
                ..
            } => self.handle_message_end(message_id, content.as_deref()).await,
            StreamEvent::ToolCallStatus {
                tool_call_id,
                status,
                error,
                display_label,
                display_summary,
                ..
            } => {
                self.handle_tool_status(
                    tool_call_id,
                    status,
                    error.as_deref(),
                    display_label.as_deref(),
                    display_summary.as_deref(),
                )
                .await
            }
            _ => Ok(()),
        }
    }

    /// Send any reply text/media not already pushed during the stream.
    pub async fn finish(&mut self, reply_text: &str) -> Result<bool> {
        let (visible_text, media_refs) = split_reply_media(reply_text);
        let mut sent_any = false;

        if !visible_text.trim().is_empty() {
            let should_send = if self.cfg.send_intermediate_text {
                visible_text.trim() != self.last_sent_visible.trim()
            } else {
                true
            };
            if should_send {
                self.plugin
                    .outbound
                    .send_text(self.outbound.clone(), &visible_text)
                    .await?;
                self.last_sent_visible = visible_text.clone();
                sent_any = true;
            }
        }

        if self.send_media_refs(&media_refs).await? {
            sent_any = true;
        }

        Ok(sent_any)
    }

    async fn handle_message_end(&mut self, message_id: &str, content: Option<&str>) -> Result<()> {
        if !self.cfg.send_intermediate_text {
            return Ok(());
        }
        if self.sent_message_ids.contains(message_id) {
            return Ok(());
        }
        let Some(raw) = content.filter(|c| !c.trim().is_empty()) else {
            return Ok(());
        };

        let sent = self.send_visible_chunk(raw).await?;
        if sent {
            self.sent_message_ids.insert(message_id.to_string());
        }
        Ok(())
    }

    async fn handle_tool_status(
        &mut self,
        tool_call_id: &str,
        status: &str,
        error: Option<&str>,
        display_label: Option<&str>,
        display_summary: Option<&str>,
    ) -> Result<()> {
        if !self.cfg.send_tool_calls {
            return Ok(());
        }
        if let Some(label) = display_label.filter(|s| !s.trim().is_empty()) {
            self.tool_display_labels
                .insert(tool_call_id.to_string(), label.to_string());
        }
        let label = resolved_tool_label(tool_call_id, display_label, &self.tool_display_labels);

        if should_skip_redundant_tool_success(
            status,
            display_summary,
            tool_call_id,
            &self.tool_started_notified,
        ) {
            return Ok(());
        }

        let key = format!("{tool_call_id}:{status}");
        if !self.notified_tool_keys.insert(key) {
            return Ok(());
        }
        let Some(line) = format_tool_status_line(status, &label, display_summary, error) else {
            return Ok(());
        };
        if matches!(status, "running" | "pending" | "pending_approval") {
            self.tool_started_notified.insert(tool_call_id.to_string());
        }
        self.plugin
            .outbound
            .send_text(self.outbound.clone(), &line)
            .await?;
        Ok(())
    }

    async fn send_visible_chunk(&mut self, raw: &str) -> Result<bool> {
        let (visible, media_refs) = split_reply_media(raw);
        let mut sent_any = false;
        if !visible.trim().is_empty() {
            self.plugin
                .outbound
                .send_text(self.outbound.clone(), &visible)
                .await?;
            self.last_sent_visible = visible.clone();
            sent_any = true;
        }
        if self.send_media_refs(&media_refs).await? {
            sent_any = true;
        }
        Ok(sent_any)
    }

    async fn send_media_refs(&mut self, media_refs: &[String]) -> Result<bool> {
        let mut sent_any = false;
        for raw_path in media_refs {
            if !self.sent_media.insert(raw_path.clone()) {
                continue;
            }
            match resolve_outbound_media(raw_path) {
                Ok(resolved) => {
                    if let Err(e) = self
                        .plugin
                        .outbound
                        .send_media(self.outbound.clone(), None, resolved.media)
                        .await
                    {
                        log::error!(
                            "channel outbound media failed conv={} path={raw_path}: {e:#}",
                            self.conv_id
                        );
                    } else {
                        sent_any = true;
                    }
                }
                Err(e) => {
                    log::error!(
                        "channel outbound media resolve failed conv={} path={raw_path}: {e:#}",
                        self.conv_id
                    );
                }
            }
        }
        Ok(sent_any)
    }
}

fn resolved_tool_label(
    tool_call_id: &str,
    display_label: Option<&str>,
    cache: &HashMap<String, String>,
) -> String {
    display_label
        .filter(|s| !s.trim().is_empty())
        .map(String::from)
        .or_else(|| cache.get(tool_call_id).cloned())
        .unwrap_or_else(|| "Tool".to_string())
}

fn should_skip_redundant_tool_success(
    status: &str,
    display_summary: Option<&str>,
    tool_call_id: &str,
    started_tools: &HashSet<String>,
) -> bool {
    status == "success"
        && display_summary.filter(|s| !s.trim().is_empty()).is_none()
        && started_tools.contains(tool_call_id)
}

fn format_tool_status_line(
    status: &str,
    label: &str,
    display_summary: Option<&str>,
    error: Option<&str>,
) -> Option<String> {
    match status {
        "running" | "pending" | "pending_approval" => {
            if let Some(summary) = display_summary.filter(|s| !s.trim().is_empty()) {
                Some(format!(
                    "🔧 {label}: {}",
                    truncate_chars(summary, TOOL_SUMMARY_MAX_CHARS)
                ))
            } else {
                Some(format!("🔧 {label}"))
            }
        }
        "success" => {
            if let Some(summary) = display_summary.filter(|s| !s.trim().is_empty()) {
                Some(format!("✓ {label}: {}", truncate_chars(summary, TOOL_SUMMARY_MAX_CHARS)))
            } else {
                Some(format!("✓ {label}"))
            }
        }
        "failed" | "rejected" | "cancelled" => {
            let detail = error.filter(|s| !s.trim().is_empty()).unwrap_or("failed");
            Some(format!("✗ {label}: {detail}"))
        }
        _ => None,
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    format!("{}…", text.chars().take(max_chars).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_status_running_line() {
        assert_eq!(
            format_tool_status_line("running", "Search", None, None).as_deref(),
            Some("🔧 Search")
        );
        assert_eq!(
            format_tool_status_line("running", "联网搜索", Some("Rust 2024"), None).as_deref(),
            Some("🔧 联网搜索: Rust 2024")
        );
    }

    #[test]
    fn tool_status_success_with_summary() {
        let line = format_tool_status_line("success", "Read file", Some("done"), None).unwrap();
        assert!(line.starts_with("✓ Read file:"));
    }

    #[test]
    fn tool_status_failed_uses_error() {
        assert_eq!(
            format_tool_status_line("failed", "Shell", None, Some("timeout")).as_deref(),
            Some("✗ Shell: timeout")
        );
    }

    #[test]
    fn resolved_tool_label_uses_cache_when_success_event_missing_label() {
        let mut cache = HashMap::new();
        cache.insert("tc1".to_string(), "写入文件".to_string());
        assert_eq!(
            resolved_tool_label("tc1", None, &cache),
            "写入文件"
        );
    }

    #[test]
    fn skip_redundant_success_after_running() {
        let mut started = HashSet::new();
        started.insert("tc1".to_string());
        assert!(should_skip_redundant_tool_success(
            "success",
            None,
            "tc1",
            &started
        ));
        assert!(!should_skip_redundant_tool_success(
            "success",
            Some("done"),
            "tc1",
            &started
        ));
    }

    #[test]
    fn truncate_chars_respects_limit() {
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
    }
}
