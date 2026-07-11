//! Task board v4 document model.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const BOARD_VERSION: u32 = 4;
pub const MAX_BOARD_ROWS: usize = 20;
pub const RESULT_SNIPPET_MAX_CHARS: usize = 800;
pub const RESULTS_MAX_ENTRIES: usize = 48;

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
    pub context: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_constraints_opt"
    )]
    pub constraints: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_when: Option<String>,
    #[serde(default)]
    pub status: MetaStatus,
    #[serde(default)]
    pub max_depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_quota: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<BoardScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_sub_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_store_key: Option<String>,
    /// Loop progress counters (synced from wi_* rows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_items_done: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_items_failed: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_items_total: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_items_in_progress: Option<u32>,
}

impl Default for BoardMeta {
    fn default() -> Self {
        Self {
            goal: String::new(),
            context: String::new(),
            constraints: None,
            done_when: None,
            status: MetaStatus::Running,
            max_depth: 0,
            expected_total: None,
            dynamic_quota: None,
            scope: None,
            root_target: None,
            parent_sub_task_id: None,
            parent_store_key: None,
            work_items_done: None,
            work_items_failed: None,
            work_items_total: None,
            work_items_in_progress: None,
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

/// Milestone row in `global_milestones`.
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
    /// User/session normative rules for this milestone (init copy; overrides Skill defaults).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_constraints_opt"
    )]
    pub constraints: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "validate_requirement"
    )]
    pub done_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
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
    #[serde(default, rename = "global_milestones", alias = "board")]
    pub global_milestones: Vec<BoardItem>,
}

impl BoardDocument {
    pub fn empty_for_store_key(store_key: &str) -> Self {
        Self {
            version: BOARD_VERSION,
            task_id: format!("tb_{store_key}"),
            meta: BoardMeta::default(),
            global_context: GlobalContext::default(),
            global_milestones: Vec::new(),
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    pub fn board_is_empty(&self) -> bool {
        self.global_milestones.is_empty() && self.meta.goal.is_empty()
    }

    pub fn is_loop_board(&self) -> bool {
        super::loop_milestones::is_loop_milestone_board(self)
    }
}

impl BoardItem {
    pub fn is_delivery_milestone(&self) -> bool {
        self.delivery_format.is_some()
    }

    pub fn has_done_when(&self) -> bool {
        self.done_when
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn has_validate_evidence(&self) -> bool {
        self.remark
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn last_remark_snippet(&self) -> Option<&str> {
        self.remark
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
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

        let done_when = str_field(v, "done_when").or_else(|| str_field(v, "validate_requirement"));

        let remark = str_field(v, "remark").or_else(|| {
            let results = string_array_field(v, "validate_results");
            results.last().cloned()
        });

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
            rules: str_field(v, "rules"),
            constraints: constraints_text_field(v, "constraints", "constraint"),
            done_when,
            remark,
            blocked_by,
            delivery_format,
        })
    }
}

/// Parse constraints text: prefer `plural` string; fall back to legacy singular `constraint`.
/// Legacy JSON arrays are joined as bullet lines.
pub fn constraints_text_field(v: &Value, plural: &str, singular: &str) -> Option<String> {
    if let Some(val) = v.get(plural) {
        if let Some(t) = constraints_text_from_value(val) {
            return Some(t);
        }
    }
    v.get(singular).and_then(constraints_text_from_value)
}

pub fn constraints_text_from_value(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr
                .iter()
                .filter_map(|e| e.as_str())
                .map(|s| compact_snippet(s.trim()))
                .filter(|s| !s.is_empty())
                .collect();
            if items.is_empty() {
                None
            } else if items.len() == 1 {
                Some(items.into_iter().next().unwrap())
            } else {
                Some(
                    items
                        .iter()
                        .map(|s| format!("- {s}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
        }
        _ => None,
    }
}

fn deserialize_constraints_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    Ok(constraints_text_from_value(&v))
}

pub fn meta_from_value(v: &Value) -> BoardMeta {
    let mut meta: BoardMeta = serde_json::from_value(v.clone()).unwrap_or_default();
    if meta
        .constraints
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        meta.constraints = constraints_text_field(v, "constraints", "constraint");
    }
    meta
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

pub fn trim_results_list(list: &mut Vec<String>) {
    while list.len() > RESULTS_MAX_ENTRIES {
        list.remove(0);
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

pub fn compact_snippet(text: &str) -> String {
    if text.chars().count() <= RESULT_SNIPPET_MAX_CHARS {
        return text.to_string();
    }
    let compact: String = text.chars().take(RESULT_SNIPPET_MAX_CHARS).collect();
    format!("{compact}…")
}

#[cfg(test)]
mod field_tests {
    use super::{constraints_text_field, BoardItem};
    use serde_json::json;

    #[test]
    fn milestone_parses_rules_and_constraints_text() {
        let v = json!({
            "id": "m1",
            "title": "Step",
            "status": "pending",
            "rules": "User normative text",
            "constraints": "- iron law A\n- iron law B"
        });
        let item = BoardItem::from_value(&v).expect("item");
        assert_eq!(item.rules.as_deref(), Some("User normative text"));
        assert_eq!(
            item.constraints.as_deref(),
            Some("- iron law A\n- iron law B")
        );
    }

    #[test]
    fn milestone_legacy_singular_constraint_coerces() {
        let v = json!({
            "id": "m2",
            "title": "Step",
            "status": "pending",
            "constraint": "legacy single"
        });
        let item = BoardItem::from_value(&v).expect("item");
        assert_eq!(item.constraints.as_deref(), Some("legacy single"));
    }

    #[test]
    fn constraints_text_field_joins_legacy_array() {
        let v = json!({
            "constraints": ["iron law A", "iron law B"],
            "constraint": "ignored when array present"
        });
        assert_eq!(
            constraints_text_field(&v, "constraints", "constraint").as_deref(),
            Some("- iron law A\n- iron law B")
        );
    }

    #[test]
    fn constraints_text_field_prefers_string_over_singular() {
        let v = json!({
            "constraints": "primary text",
            "constraint": "legacy singular"
        });
        assert_eq!(
            constraints_text_field(&v, "constraints", "constraint").as_deref(),
            Some("primary text")
        );
    }
}
