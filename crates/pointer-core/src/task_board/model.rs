//! Task board v3 document model.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const BOARD_VERSION: u32 = 3;
pub const MAX_BOARD_ROWS: usize = 20;
pub const RESULT_SNIPPET_MAX_CHARS: usize = 800;
pub const RESULTS_MAX_ENTRIES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemMode {
    Enumerated,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryFormat {
    Xlsx,
    Csv,
    Txt,
    Jsonl,
}

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

impl Default for BoardMeta {
    fn default() -> Self {
        Self {
            goal: String::new(),
            status: MetaStatus::Running,
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
    pub plan: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "checkpoint"
    )]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate_requirement: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validate_results: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_requirement: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extract_results: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_mode: Option<WorkItemMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_quota: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_format: Option<DeliveryFormat>,
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
    pub fn has_work_items(&self) -> bool {
        self.work_item_mode.is_some() || self.dynamic_quota.is_some()
    }

    pub fn is_delivery_milestone(&self) -> bool {
        self.delivery_format.is_some() && !self.has_work_items()
    }

    pub fn is_dynamic_work_items(&self) -> bool {
        self.work_item_mode == Some(WorkItemMode::Dynamic) || self.dynamic_quota.is_some()
    }

    pub fn is_enumerated_work_items(&self) -> bool {
        self.work_item_mode == Some(WorkItemMode::Enumerated)
    }

    pub fn has_validate_evidence(&self) -> bool {
        self.validate_results.iter().any(|s| !s.trim().is_empty())
    }

    pub fn last_validate_result_snippet(&self) -> Option<&str> {
        self.validate_results
            .iter()
            .rev()
            .find_map(|s| {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
    }

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
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let blocked_by = v
            .get("blocked_by")
            .and_then(|x| x.as_str())
            .map(str::to_string);

        let work_item_mode = v
            .get("work_item_mode")
            .and_then(|x| x.as_str())
            .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
                "enumerated" => Some(WorkItemMode::Enumerated),
                "dynamic" => Some(WorkItemMode::Dynamic),
                _ => None,
            });
        let dynamic_quota = v
            .get("dynamic_quota")
            .and_then(value_to_u32_loose);
        let delivery_format = v
            .get("delivery_format")
            .and_then(|x| x.as_str())
            .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
                "xlsx" => Some(DeliveryFormat::Xlsx),
                "csv" => Some(DeliveryFormat::Csv),
                "txt" => Some(DeliveryFormat::Txt),
                "jsonl" => Some(DeliveryFormat::Jsonl),
                _ => None,
            });

        Some(Self {
            id,
            title,
            status,
            depends_on,
            retry_count,
            plan: str_field(v, "plan"),
            progress: str_field(v, "progress").or_else(|| str_field(v, "checkpoint")),
            validate_requirement: str_field(v, "validate_requirement"),
            validate_results: string_array_field(v, "validate_results"),
            extract_requirement: str_field(v, "extract_requirement"),
            extract_results: string_array_field(v, "extract_results"),
            blocked_by,
            work_item_mode,
            dynamic_quota,
            delivery_format,
        })
    }
}

fn value_to_u32_loose(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok()))
            .map(|u| u as u32),
        Value::String(s) => s.trim().parse::<u32>().ok(),
        _ => None,
    }
}

pub fn str_field(v: &Value, key: &str) -> Option<String> {
    let s = v.get(key)?.as_str()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn string_array_field(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .map(parse_string_array)
        .unwrap_or_default()
}

pub fn parse_string_array(val: &Value) -> Vec<String> {
    match val {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                vec![compact_snippet(t)]
            }
        }
        Value::Array(arr) => arr
            .iter()
            .filter_map(|e| e.as_str())
            .map(|s| compact_snippet(s.trim()))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

pub fn push_snippet(list: &mut Vec<String>, text: &str) {
    let t = compact_snippet(text.trim());
    if t.is_empty() {
        return;
    }
    list.push(t);
    trim_results_list(list);
}

pub fn append_snippets_from_value(list: &mut Vec<String>, v: &Value, key: &str) {
    let Some(val) = v.get(key) else {
        return;
    };
    for s in parse_string_array(val) {
        push_snippet(list, &s);
    }
}

pub fn compact_snippet(text: &str) -> String {
    if text.chars().count() <= RESULT_SNIPPET_MAX_CHARS {
        return text.to_string();
    }
    let compact: String = text.chars().take(RESULT_SNIPPET_MAX_CHARS).collect();
    format!("{compact}…")
}

pub fn trim_results_list(list: &mut Vec<String>) {
    while list.len() > RESULTS_MAX_ENTRIES {
        list.remove(0);
    }
}

