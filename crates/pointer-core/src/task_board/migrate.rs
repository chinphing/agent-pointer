//! Load persisted task board JSON as v4 [`BoardDocument`].

use super::model::{
    BoardDocument, BoardItem, BoardMeta, WorkItemMode, BOARD_VERSION,
};
use serde_json::Value;

const V3_VERSION: u32 = 3;

pub fn normalize_stored_value(store_key: &str, raw: Value) -> BoardDocument {
    let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version == V3_VERSION {
        return migrate_v3_value(store_key, &raw);
    }
    if let Ok(doc) = serde_json::from_value::<BoardDocument>(raw.clone()) {
        if doc.version == BOARD_VERSION && !doc.task_id.is_empty() {
            return doc;
        }
        if doc.version != BOARD_VERSION {
            log::warn!(
                "task_board: ignoring stored document version={} (expected {BOARD_VERSION}) store_key={store_key}",
                doc.version,
            );
        }
    }
    // Legacy: bare array of rows
    if let Some(arr) = raw.as_array() {
        let mut doc = BoardDocument::empty_for_store_key(store_key);
        for v in arr {
            if let Some(item) = BoardItem::from_value(v) {
                doc.global_milestones.push(item);
            }
        }
        if !doc.global_milestones.is_empty() {
            return doc;
        }
    }
    BoardDocument::empty_for_store_key(store_key)
}

/// Deserialize v3 raw JSON into v4 document (handles v3-only fields on rows).
pub fn migrate_v3_value(store_key: &str, raw: &Value) -> BoardDocument {
    let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if version == BOARD_VERSION {
        return serde_json::from_value(raw.clone())
            .unwrap_or_else(|_| BoardDocument::empty_for_store_key(store_key));
    }
    let mut doc = BoardDocument::empty_for_store_key(store_key);
    doc.task_id = raw
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("tb_{store_key}"));
    if let Some(meta_v) = raw.get("meta") {
        if let Ok(m) = serde_json::from_value::<BoardMeta>(meta_v.clone()) {
            doc.meta = m;
        }
    }
    if let Some(gc) = raw.get("global_context") {
        if let Ok(g) = serde_json::from_value(gc.clone()) {
            doc.global_context = g;
        }
    }
    let rows = raw
        .get("board")
        .or_else(|| raw.get("global_milestones"))
        .and_then(|v| v.as_array());
    if let Some(arr) = rows {
        for v in arr {
            if let Some(mut item) = BoardItem::from_value(v) {
                if let Some(mode_s) = v.get("work_item_mode").and_then(|x| x.as_str()) {
                    doc.meta.work_item_mode = match mode_s.trim().to_ascii_lowercase().as_str() {
                        "enumerated" => Some(WorkItemMode::Enumerated),
                        "dynamic" => Some(WorkItemMode::Dynamic),
                        _ => doc.meta.work_item_mode,
                    };
                }
                if let Some(q) = v.get("dynamic_quota").and_then(|x| x.as_u64()) {
                    doc.meta.dynamic_quota = Some(q as u32);
                }
                if item.remark.is_none() {
                    if let Some(results) = v.get("validate_results") {
                        let arr = super::model::parse_string_array(results);
                        if let Some(last) = arr.last() {
                            item.remark = Some(last.clone());
                        }
                    }
                }
                doc.global_milestones.push(item);
            }
        }
    }
    if let Some(arr) = raw.get("item_milestones").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(item) = BoardItem::from_value(v) {
                doc.item_milestones.push(item);
            }
        }
    }
    log::info!(
        "task_board: migrated v3→v4 store_key={store_key} global_rows={}",
        doc.global_milestones.len()
    );
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn migrates_v3_board_to_global_milestones() {
        let raw = json!({
            "version": 3,
            "task_id": "tb_conv",
            "meta": { "goal": "g" },
            "board": [
                {
                    "id": "m1",
                    "title": "Step",
                    "status": "in_progress",
                    "validate_requirement": "tests pass",
                    "validate_results": ["ok"]
                }
            ]
        });
        let doc = migrate_v3_value("conv", &raw);
        assert_eq!(doc.version, BOARD_VERSION);
        assert_eq!(doc.global_milestones.len(), 1);
        assert_eq!(
            doc.global_milestones[0].done_when.as_deref(),
            Some("tests pass")
        );
        assert_eq!(doc.global_milestones[0].remark.as_deref(), Some("ok"));
    }

    #[test]
    fn non_v3_stored_value_returns_empty_board() {
        let raw = json!({"unexpected": true});
        let doc = normalize_stored_value("k", raw);
        assert_eq!(doc.version, BOARD_VERSION);
        assert!(doc.global_milestones.is_empty());
    }

    #[test]
    fn legacy_array_migrates_rows() {
        let raw = json!([{"id":"1","title":"t","status":"done"}]);
        let doc = normalize_stored_value("k", raw);
        assert_eq!(doc.global_milestones.len(), 1);
    }
}
