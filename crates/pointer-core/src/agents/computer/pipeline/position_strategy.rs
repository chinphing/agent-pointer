//! Position output shape, JSON schema, and decision→execution bridging per tool family.
//!
//! Each spatial family picks a [`PositionSpatialModel`]. [`merge_position_output`] writes
//! execution-ready arg fields directly — no separate post-normalize adapter pass.

use super::operation::{valid_overlay_index, OperationFamily};
use super::types::{PositionModuleOutput, PositionPoint, PositionSubmitRoute};
use serde_json::{json, Value};

/// How many spatial anchors the Position LLM may output for a family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionSpatialModel {
    /// `{ index }` or `{ x, y, reference_index }`
    SingleIndexOrAt,
    /// `{ index }` or `{ x, y }`
    SingleIndexOrXy,
    /// `{ indices: [N,…] }` or `{ positions: [{x,y},…] }`
    MultipleIndexOrXy,
}

impl OperationFamily {
    /// Position spatial model for families that run the Position LLM.
    pub fn position_spatial_model(self) -> Option<PositionSpatialModel> {
        match self {
            Self::PointerClick | Self::PointerHover | Self::Input => {
                Some(PositionSpatialModel::SingleIndexOrAt)
            }
            Self::Scroll | Self::Captcha => Some(PositionSpatialModel::SingleIndexOrXy),
            Self::Drag | Self::ModifiedClick => Some(PositionSpatialModel::MultipleIndexOrXy),
            _ => None,
        }
    }
}

pub fn position_schema_for_family(family: OperationFamily) -> Value {
    match family.position_spatial_model() {
        Some(PositionSpatialModel::SingleIndexOrAt) => schema_single_index_or_at(),
        Some(PositionSpatialModel::SingleIndexOrXy) => schema_single_index_or_xy(),
        Some(PositionSpatialModel::MultipleIndexOrXy) => schema_multiple_index_or_xy(),
        None => json!({
            "type": "object",
            "additionalProperties": true
        }),
    }
}

fn schema_single_index_route() -> Value {
    json!({
        "type": "object",
        "properties": {
            "index": { "type": "integer", "minimum": 1 }
        },
        "required": ["index"],
        "additionalProperties": false
    })
}

fn schema_at_route_with_reference() -> Value {
    json!({
        "type": "object",
        "properties": {
            "x": { "type": "integer" },
            "y": { "type": "integer" },
            "reference_index": { "type": "integer", "minimum": 1 }
        },
        "required": ["x", "y", "reference_index"],
        "additionalProperties": false
    })
}

fn schema_xy_route() -> Value {
    json!({
        "type": "object",
        "properties": {
            "x": { "type": "integer" },
            "y": { "type": "integer" }
        },
        "required": ["x", "y"],
        "additionalProperties": false
    })
}

fn schema_indices_route() -> Value {
    json!({
        "type": "object",
        "properties": {
            "indices": {
                "type": "array",
                "items": { "type": "integer", "minimum": 1 },
                "minItems": 1
            }
        },
        "required": ["indices"],
        "additionalProperties": false
    })
}

fn schema_positions_route() -> Value {
    json!({
        "type": "object",
        "properties": {
            "positions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "integer" },
                        "y": { "type": "integer" }
                    },
                    "required": ["x", "y"],
                    "additionalProperties": false
                },
                "minItems": 1
            }
        },
        "required": ["positions"],
        "additionalProperties": false
    })
}

fn schema_single_index_or_at() -> Value {
    json!({
        "oneOf": [schema_single_index_route(), schema_at_route_with_reference()]
    })
}

fn schema_single_index_or_xy() -> Value {
    json!({
        "oneOf": [schema_single_index_route(), schema_xy_route()]
    })
}

fn schema_multiple_index_or_xy() -> Value {
    json!({
        "oneOf": [schema_indices_route(), schema_positions_route()]
    })
}

/// Virtual pipeline tools for Position LLM (flat parameters per route — no oneOf).
pub fn position_submit_tools(family: OperationFamily) -> Vec<Value> {
    fn pipeline_tool(name: &str, description: &str, parameters: Value) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description,
                "parameters": parameters
            }
        })
    }
    match family.position_spatial_model() {
        Some(PositionSpatialModel::SingleIndexOrAt) => vec![
            pipeline_tool(
                "submit_position_index",
                "Submit overlay index after inner-center-wrap proof (Steps 1-4 in thinking).",
                schema_single_index_route(),
            ),
            pipeline_tool(
                "submit_position_at",
                "Submit session coordinates after inner-edge-wrap or unwrapped proof (Steps 1-4 in thinking).",
                schema_at_route_with_reference(),
            ),
        ],
        Some(PositionSpatialModel::SingleIndexOrXy) => vec![
            pipeline_tool(
                "submit_position_index",
                "Submit scrollable panel overlay index after spatial proof.",
                schema_single_index_route(),
            ),
            pipeline_tool(
                "submit_position_xy",
                "Submit pointer anchor (x, y) on the scroll region after spatial proof.",
                schema_xy_route(),
            ),
        ],
        Some(PositionSpatialModel::MultipleIndexOrXy) => vec![
            pipeline_tool(
                "submit_position_indices",
                "Submit overlay index list after proving each anchor (index route).",
                schema_indices_route(),
            ),
            pipeline_tool(
                "submit_position_positions",
                "Submit session coordinate list after proving each anchor (at route).",
                schema_positions_route(),
            ),
        ],
        None => Vec::new(),
    }
}

/// Target overlay index from the index route only (`submit_position_index`).
/// `reference_index` on the at route is a spatial anchor — not a click target.
fn position_overlay_index(pos: &PositionModuleOutput) -> Option<u32> {
    pos.index.filter(|i| valid_overlay_index(*i))
}

fn has_at_coordinates(pos: &PositionModuleOutput) -> bool {
    pos.x.is_some() && pos.y.is_some()
}

fn collect_indices(pos: &PositionModuleOutput) -> Vec<u32> {
    if let Some(list) = pos.indices.as_ref().filter(|v| !v.is_empty()) {
        return list
            .iter()
            .copied()
            .filter(|i| valid_overlay_index(*i))
            .collect();
    }
    if let (Some(from), Some(to)) = (
        pos.from_index.filter(|i| valid_overlay_index(*i)),
        pos.to_index.filter(|i| valid_overlay_index(*i)),
    ) {
        return vec![from, to];
    }
    if let Some(i) = position_overlay_index(pos) {
        return vec![i];
    }
    Vec::new()
}

fn collect_positions(pos: &PositionModuleOutput) -> Vec<PositionPoint> {
    if let Some(list) = pos.positions.as_ref().filter(|v| !v.is_empty()) {
        return list.clone();
    }
    if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (pos.x1, pos.y1, pos.x2, pos.y2) {
        return vec![
            PositionPoint { x: x1, y: y1 },
            PositionPoint { x: x2, y: y2 },
        ];
    }
    if let (Some(x), Some(y)) = (pos.x, pos.y) {
        return vec![PositionPoint { x, y }];
    }
    Vec::new()
}

/// Merge Position LLM fields into execution-ready tool args.
pub fn merge_position_output(
    family: OperationFamily,
    args: &mut Value,
    pos: &PositionModuleOutput,
) {
    let Some(model) = family.position_spatial_model() else {
        return;
    };
    let Some(obj) = args.as_object_mut() else {
        return;
    };
    match model {
        PositionSpatialModel::SingleIndexOrAt | PositionSpatialModel::SingleIndexOrXy => {
            merge_single_point(obj, pos);
        }
        PositionSpatialModel::MultipleIndexOrXy => merge_multiple_index_or_xy(family, obj, pos),
    }
}

fn merge_single_point(obj: &mut serde_json::Map<String, Value>, pos: &PositionModuleOutput) {
    match pos.submit_route {
        Some(PositionSubmitRoute::Index) => {
            if let Some(index) = pos.index.filter(|i| valid_overlay_index(*i)) {
                obj.insert("index".into(), json!(index));
            } else if pos.index == Some(0) {
                log::warn!(
                    "computer pipeline: position returned invalid overlay index 0 — keeping decision index {:?}",
                    obj.get("index")
                );
            }
        }
        Some(PositionSubmitRoute::At) | Some(PositionSubmitRoute::Xy) => {
            if let Some(x) = pos.x {
                obj.insert("x".into(), json!(x));
            }
            if let Some(y) = pos.y {
                obj.insert("y".into(), json!(y));
            }
        }
        _ => merge_single_point_inferred(obj, pos),
    }
}

fn merge_single_point_inferred(
    obj: &mut serde_json::Map<String, Value>,
    pos: &PositionModuleOutput,
) {
    if let Some(x) = pos.x {
        obj.insert("x".into(), json!(x));
    }
    if let Some(y) = pos.y {
        obj.insert("y".into(), json!(y));
    }
    if let Some(index) = position_overlay_index(pos) {
        obj.insert("index".into(), json!(index));
    } else if pos.index == Some(0) || pos.reference_index == Some(0) {
        log::warn!(
            "computer pipeline: position returned invalid overlay index 0 — keeping decision index {:?}",
            obj.get("index")
        );
    }
}

fn merge_multiple_index_or_xy(
    family: OperationFamily,
    obj: &mut serde_json::Map<String, Value>,
    pos: &PositionModuleOutput,
) {
    let use_index_route = matches!(
        pos.submit_route,
        Some(PositionSubmitRoute::Indices) | Some(PositionSubmitRoute::Index)
    );
    let use_at_route = matches!(
        pos.submit_route,
        Some(PositionSubmitRoute::Positions)
            | Some(PositionSubmitRoute::At)
            | Some(PositionSubmitRoute::Xy)
    );

    let indices = if use_at_route {
        Vec::new()
    } else {
        collect_indices(pos)
    };
    if !indices.is_empty() {
        match family {
            OperationFamily::Drag => write_drag_indices(obj, &indices),
            OperationFamily::ModifiedClick => {
                obj.insert("indices".into(), json!(indices));
            }
            _ => {
                obj.insert("indices".into(), json!(indices));
            }
        }
        return;
    }

    let positions = if use_index_route {
        Vec::new()
    } else {
        collect_positions(pos)
    };
    if !positions.is_empty() {
        match family {
            OperationFamily::Drag => write_drag_positions(obj, &positions),
            OperationFamily::ModifiedClick => {
                obj.insert("positions".into(), json!(positions));
            }
            _ => {
                obj.insert("positions".into(), json!(positions));
            }
        }
    }
}

fn write_drag_indices(obj: &mut serde_json::Map<String, Value>, indices: &[u32]) {
    if indices.len() >= 2 {
        obj.insert("from_index".into(), json!(indices[0]));
        obj.insert("to_index".into(), json!(indices[1]));
        log::info!(
            "computer pipeline: merged drag indices [{}, {}] → from_index/to_index",
            indices[0],
            indices[1]
        );
    } else if indices.len() == 1 {
        obj.insert("from_index".into(), json!(indices[0]));
        log::warn!("computer pipeline: drag indices has only one entry — missing to_index");
    }
}

fn write_drag_positions(obj: &mut serde_json::Map<String, Value>, positions: &[PositionPoint]) {
    if positions.len() >= 2 {
        obj.insert("x1".into(), json!(positions[0].x));
        obj.insert("y1".into(), json!(positions[0].y));
        obj.insert("x2".into(), json!(positions[1].x));
        obj.insert("y2".into(), json!(positions[1].y));
    } else if let Some(p) = positions.first() {
        obj.insert("x1".into(), json!(p.x));
        obj.insert("y1".into(), json!(p.y));
        log::warn!("computer pipeline: drag positions has only one entry — missing destination");
    }
}

/// Decision tool name + position output → flat execution tool name.
pub fn resolve_execution_tool(decision_name: &str, pos: &PositionModuleOutput) -> String {
    let n = decision_name.trim().to_ascii_lowercase();
    let (has_index, has_at) = route_flags_from_submit_tool(pos);
    let indices = collect_indices(pos);
    let exec_name: &str = match n.as_str() {
        "click" => route_index_or_at(has_index, has_at, "mouse_click_index", "mouse_click_at"),
        "double_click" => route_index_or_at(
            has_index,
            has_at,
            "mouse_double_click_index",
            "mouse_double_click_at",
        ),
        "right_click" => route_index_or_at(
            has_index,
            has_at,
            "mouse_right_click_index",
            "mouse_right_click_at",
        ),
        "hover" | "mouse_move" => {
            route_index_or_at(has_index, has_at, "mouse_hover_index", "mouse_hover_at")
        }
        "scroll" => route_index_or_at(
            has_index,
            has_at,
            "mouse_scroll_index",
            "mouse_scroll_current",
        ),
        "drag" => route_drag_indices_or_at(&indices, has_at),
        "input" => route_index_or_at(has_index, has_at, "input_index", "input_at"),
        "modified_click" => route_modified_click(&indices, has_at),
        other => other,
    };
    exec_name.to_string()
}

/// Route flags from the Position submit tool name (primary), with field-shape fallback for tests.
fn route_flags_from_submit_tool(pos: &PositionModuleOutput) -> (bool, bool) {
    match pos.submit_route {
        Some(PositionSubmitRoute::Index) | Some(PositionSubmitRoute::Indices) => (true, false),
        Some(PositionSubmitRoute::At)
        | Some(PositionSubmitRoute::Xy)
        | Some(PositionSubmitRoute::Positions) => (false, true),
        Some(PositionSubmitRoute::Unknown) | None => {
            let indices = collect_indices(pos);
            let has_at = !collect_positions(pos).is_empty() || has_at_coordinates(pos);
            let has_index =
                !has_at && (!indices.is_empty() || position_overlay_index(pos).is_some());
            (has_index, has_at)
        }
    }
}

fn route_index_or_at(
    has_index: bool,
    has_at: bool,
    index_tool: &'static str,
    at_tool: &'static str,
) -> &'static str {
    if has_at {
        at_tool
    } else if has_index {
        index_tool
    } else {
        at_tool
    }
}

fn route_drag_indices_or_at(indices: &[u32], has_at: bool) -> &'static str {
    if indices.len() >= 2 || indices.len() == 1 {
        "mouse_drag_from_to_index"
    } else if has_at {
        "mouse_drag_from_to_at"
    } else {
        "mouse_drag_from_to_at"
    }
}

fn route_modified_click(indices: &[u32], has_at: bool) -> &'static str {
    if !indices.is_empty() {
        if indices.len() == 2 {
            "modified_click_range_select_index"
        } else {
            "modified_click_select_index"
        }
    } else if has_at {
        "modified_click_select_at"
    } else {
        "modified_click_select_at"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_map_to_spatial_models() {
        assert_eq!(
            OperationFamily::PointerClick.position_spatial_model(),
            Some(PositionSpatialModel::SingleIndexOrAt)
        );
        assert_eq!(
            OperationFamily::Drag.position_spatial_model(),
            Some(PositionSpatialModel::MultipleIndexOrXy)
        );
        assert_eq!(
            OperationFamily::ModifiedClick.position_spatial_model(),
            Some(PositionSpatialModel::MultipleIndexOrXy)
        );
        assert_eq!(OperationFamily::Wait.position_spatial_model(), None);
    }

    #[test]
    fn schema_drag_uses_multiple_index_or_xy() {
        let schema = position_schema_for_family(OperationFamily::Drag);
        let one_of = schema.get("oneOf").and_then(|v| v.as_array()).unwrap();
        assert_eq!(one_of.len(), 2);
        assert!(one_of[0]
            .get("required")
            .unwrap()
            .as_array()
            .unwrap()
            .contains(&json!("indices")));
    }

    #[test]
    fn merge_modified_click_writes_indices() {
        let mut args = json!({"goal": "g"});
        let pos = PositionModuleOutput {
            indices: Some(vec![42]),
            ..Default::default()
        };
        merge_position_output(OperationFamily::ModifiedClick, &mut args, &pos);
        assert_eq!(args["indices"], json!([42]));
        assert!(args.get("index").is_none());
    }

    #[test]
    fn merge_drag_indices_to_from_to() {
        let mut args = json!({"goal": "g"});
        let pos = PositionModuleOutput {
            indices: Some(vec![3, 7]),
            ..Default::default()
        };
        merge_position_output(OperationFamily::Drag, &mut args, &pos);
        assert_eq!(args["from_index"], 3);
        assert_eq!(args["to_index"], 7);
    }

    #[test]
    fn merge_modified_click_legacy_single_index() {
        let mut args = json!({"goal": "g"});
        let pos = PositionModuleOutput {
            index: Some(42),
            ..Default::default()
        };
        merge_position_output(OperationFamily::ModifiedClick, &mut args, &pos);
        assert_eq!(args["indices"], json!([42]));
    }

    #[test]
    fn resolve_modified_click_range_for_two_indices() {
        let pos = PositionModuleOutput {
            indices: Some(vec![1, 5]),
            ..Default::default()
        };
        assert_eq!(
            resolve_execution_tool("modified_click", &pos),
            "modified_click_range_select_index"
        );
    }

    #[test]
    fn at_route_reference_index_does_not_route_to_click_index() {
        let pos = PositionModuleOutput {
            submit_route: Some(PositionSubmitRoute::At),
            x: Some(279),
            y: Some(332),
            reference_index: Some(83),
            ..Default::default()
        };
        assert_eq!(resolve_execution_tool("click", &pos), "mouse_click_at");
        let mut args = json!({"action": "Click copy icon"});
        merge_position_output(OperationFamily::PointerClick, &mut args, &pos);
        assert_eq!(args["x"], 279);
        assert_eq!(args["y"], 332);
        assert!(args.get("index").is_none());
    }
}
