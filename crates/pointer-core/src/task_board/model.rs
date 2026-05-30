//! Task board v2 document model.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const BOARD_VERSION: u32 = 2;
pub const DEFAULT_MAX_STEPS: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardScope {
    Parent,
    Child,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaStatus {
    Running,
    Completed,
    Failed,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    #[default]
    Pending,
    Ready,
    InProgress,
    Done,
    Cancelled,
    Failed,
}

impl ItemStatus {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "in_progress" | "running" => Some(Self::InProgress),
            "done" | "success" => Some(Self::Done),
            "cancelled" | "canceled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardMeta {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub status: MetaStatus,
    #[serde(default)]
    pub step_count: u32,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
    #[serde(default)]
    pub max_depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<BoardScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_sub_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_store_key: Option<String>,
}

fn default_max_steps() -> u32 {
    DEFAULT_MAX_STEPS
}

impl Default for BoardMeta {
    fn default() -> Self {
        Self {
            goal: String::new(),
            status: MetaStatus::Running,
            step_count: 0,
            max_steps: DEFAULT_MAX_STEPS,
            max_depth: 0,
            expected_total: None,
            scope: None,
            root_target: None,
            parent_sub_task_id: None,
            parent_store_key: None,
        }
    }
}

impl Default for MetaStatus {
    fn default() -> Self {
        Self::Running
    }
}

impl MetaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Paused => "paused",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalContext {
    #[serde(default)]
    pub key_findings: Vec<String>,
    #[serde(default)]
    pub artifacts: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BoardItem {
    pub id: String,
    pub title: String,
    pub status: ItemStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detailed_plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardDocument {
    pub version: u32,
    pub task_id: String,
    pub meta: BoardMeta,
    #[serde(default)]
    pub global_context: GlobalContext,
    #[serde(default)]
    pub board: Vec<BoardItem>,
}

impl BoardDocument {
    pub fn empty_for_store_key(store_key: &str) -> Self {
        Self {
            version: BOARD_VERSION,
            task_id: format!("tb_{store_key}"),
            meta: BoardMeta::default(),
            global_context: GlobalContext::default(),
            board: Vec::new(),
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    pub fn board_is_empty(&self) -> bool {
        self.board.is_empty() && self.meta.goal.is_empty()
    }
}

impl BoardItem {
    pub fn from_value(v: &Value) -> Option<Self> {
        let id = v.get("id")?.as_str()?.trim().to_string();
        if id.is_empty() {
            return None;
        }
        let title = v
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let status = v
            .get("status")
            .and_then(|x| x.as_str())
            .and_then(ItemStatus::from_str_loose)
            .unwrap_or(ItemStatus::Pending);
        let depends_on = v
            .get("depends_on")
            .or_else(|| v.get("dependsOn"))
            .and_then(|x| {
                if let Some(arr) = x.as_array() {
                    Some(
                        arr.iter()
                            .filter_map(|e| e.as_str().map(str::trim).filter(|s| !s.is_empty()))
                            .map(str::to_string)
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let retry_count = v
            .get("retry_count")
            .or_else(|| v.get("retryCount"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let output = v
            .get("output")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let detailed_plan = v
            .get("detailed_plan")
            .or_else(|| v.get("detailedPlan"))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let verification = v
            .get("verification")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let blocked_by = v
            .get("blockedBy")
            .or_else(|| v.get("blocked_by"))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        Some(Self {
            id,
            title,
            status,
            depends_on,
            retry_count,
            output,
            detailed_plan,
            verification,
            blocked_by,
        })
    }
}
