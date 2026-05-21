//! 本地持久化 LLM token 用量，待上报至 Openpointer。

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use uuid::Uuid;

use crate::llm_token_stats::ConversationLlmStats;
use crate::storage::app_data_dir;

const PENDING_FILE: &str = "token_usage_pending.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTokenUsageReport {
    pub request_id: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub thinking_tokens: u32,
    pub total_tokens: u32,
    pub model_name: Option<String>,
    pub assistant_rounds: Option<u32>,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

fn pending_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(PENDING_FILE))
}

pub fn enqueue_from_stats(
    stats: &ConversationLlmStats,
    conversation_id: &str,
    model_name: Option<String>,
) -> Result<()> {
    if stats.llm_rounds == 0 {
        return Ok(());
    }
    let thinking = stats.sum_reasoning.min(u64::from(u32::MAX)) as u32;
    let completion = stats.sum_completion.min(u64::from(u32::MAX)) as u32;
    let prompt = stats.sum_prompt.min(u64::from(u32::MAX)) as u32;
    let total = stats.sum_total.min(u64::from(u32::MAX)) as u32;
    let now = Utc::now().to_rfc3339();
    let row = PendingTokenUsageReport {
        request_id: format!("{conversation_id}-{}", Uuid::new_v4()),
        prompt_tokens: prompt,
        completion_tokens: completion.saturating_sub(thinking),
        thinking_tokens: thinking,
        total_tokens: total,
        model_name,
        assistant_rounds: Some(stats.llm_rounds),
        period_start: Some(now.clone()),
        period_end: Some(now),
    };
    append_pending(&row)
}

fn append_pending(row: &PendingTokenUsageReport) -> Result<()> {
    let path = pending_path()?;
    let line = serde_json::to_string(row)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(f, "{line}")?;
    f.flush()?;
    log::info!(
        "token_usage_queue: enqueued request_id={} total_tokens={}",
        row.request_id,
        row.total_tokens
    );
    Ok(())
}

pub fn read_all_pending() -> Result<Vec<PendingTokenUsageReport>> {
    let path = pending_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = File::open(&path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match serde_json::from_str::<PendingTokenUsageReport>(t) {
            Ok(r) => out.push(r),
            Err(e) => log::warn!("token_usage_queue: skip bad line: {e}"),
        }
    }
    Ok(out)
}

pub fn rewrite_pending(remaining: &[PendingTokenUsageReport]) -> Result<()> {
    let path = pending_path()?;
    if remaining.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    let mut f = File::create(&path)?;
    for row in remaining {
        writeln!(f, "{}", serde_json::to_string(row)?)?;
    }
    Ok(())
}

pub async fn flush_pending_reports(
    auth: &crate::platform_auth::PlatformAuthManager,
) -> Result<usize> {
    let pending = read_all_pending()?;
    if pending.is_empty() {
        return Ok(0);
    }
    let mut remaining = Vec::new();
    let mut sent = 0usize;
    for row in pending {
        let body = serde_json::json!({
            "prompt_tokens": row.prompt_tokens,
            "completion_tokens": row.completion_tokens,
            "thinking_tokens": row.thinking_tokens,
            "total_tokens": row.total_tokens,
            "model_name": row.model_name,
            "request_id": row.request_id,
            "assistant_rounds": row.assistant_rounds,
            "period_start": row.period_start,
            "period_end": row.period_end,
        });
        match auth.report_token_usage(body).await {
            Ok(()) => {
                sent += 1;
            }
            Err(e) => {
                log::warn!(
                    "token_usage_queue: report failed request_id={}: {e}",
                    row.request_id
                );
                remaining.push(row);
            }
        }
    }
    rewrite_pending(&remaining)?;
    if sent > 0 {
        log::info!("token_usage_queue: flushed {sent} report(s)");
    }
    Ok(sent)
}
