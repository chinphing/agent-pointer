//! External work item rows (Campaign-aligned subset for task board v1).

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RESULT_SUMMARY_INJECT_MAX: usize = 512;
pub const MAX_INLINE_SEED: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Pending,
    Ready,
    InProgress,
    Done,
    Failed,
    Cancelled,
}

impl WorkItemStatus {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "in_progress" | "running" => Some(Self::InProgress),
            "done" | "success" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "cancelled" | "canceled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkItemDraft {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: String,
    #[serde(alias = "campaign_id")]
    pub store_id: String,
    pub seq: i64,
    pub status: WorkItemStatus,
    pub title: String,
    pub payload_json: String,
    #[serde(default)]
    pub retry_count: i32,
    #[serde(default)]
    pub max_retries: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    pub in_progress: u32,
    pub pending: u32,
}

impl BatchStats {
    pub fn progress_label(&self) -> String {
        let total = self
            .total
            .max(self.done + self.failed + self.in_progress + self.pending);
        format!("{}/{} done", self.done, total)
    }

    pub fn is_batch_terminal(&self) -> bool {
        self.total > 0
            && self.in_progress == 0
            && self.done + self.failed + self.pending >= self.total
            && self.pending == 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct StoreStats {
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    pub in_progress: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SeedOutcome {
    pub seeded: u32,
}

pub fn work_item_id(store_id: &str, seq: i64) -> String {
    format!("wi_{store_id}_{seq:06}")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn payload_with_target_key(mut payload: Value, target_key: Option<String>) -> Value {
    if let Some(ref key) = target_key {
        if !key.trim().is_empty() {
            if payload.is_null() {
                payload = serde_json::json!({ "target_key": key });
            } else if let Some(obj) = payload.as_object_mut() {
                obj.entry("target_key".to_string())
                    .or_insert_with(|| Value::String(key.clone()));
            }
        }
    }
    payload
}

pub fn target_key_from_payload(payload_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(payload_json).ok()?;
    v.get("target_key")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn merge_result_summary(existing: Option<&str>, summary: &str) -> String {
    let t = summary.trim();
    if t.is_empty() {
        return existing.unwrap_or("{}").to_string();
    }
    let compact = if t.chars().count() > RESULT_SUMMARY_INJECT_MAX {
        let c: String = t.chars().take(RESULT_SUMMARY_INJECT_MAX).collect();
        format!("{c}…")
    } else {
        t.to_string()
    };
    serde_json::json!({ "summary": compact }).to_string()
}

pub fn result_summary_from_json(result_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(result_json).ok()?;
    if let Some(s) = v.get("summary").and_then(|x| x.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if v.is_string() {
        return v
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    None
}

pub fn draft_from_value(v: &Value) -> Option<WorkItemDraft> {
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let payload = v
        .get("payload")
        .cloned()
        .or_else(|| v.get("payload_json").cloned())
        .unwrap_or(Value::Null);
    let target_key = v
        .get("target_key")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| target_key_from_payload(&payload.to_string()));
    Some(WorkItemDraft {
        title,
        payload,
        target_key,
    })
}
