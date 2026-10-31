//! Position output shape, JSON schema, and decision→execution bridging per tool family.
//!
//! Each spatial family picks a [`PositionSpatialModel`]. [`merge_position_output`] writes
//! execution-ready arg fields directly — no separate post-normalize adapter pass.
//!
//! **Coordinates only.** The overlay numbering that used to back an index route was
//! removed in P5: `screen_overlay` no longer draws digits, and the `*_index` tools are
//! gone. Everything here reports `(x, y)`, and the only thing the position model can
//! submit is a point or a list of points.

use super::operation::OperationFamily;
use super::types::{PositionModuleOutput, PositionPoint};
use serde_json::{json, Value};

/// How many spatial anchors the Position LLM may output for a family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionSpatialModel {
    /// One point.
    SingleAt,
    /// One point, reported as `{ x, y }`.
    SingleXy,
    /// `{ positions: [{x,y},…] }`.
    MultipleXy,
}

impl OperationFamily {
    /// Position spatial model for families that run the Position LLM.
    pub fn position_spatial_model(self) -> Option<PositionSpatialModel> {
        match self {
            Self::PointerClick | Self::PointerHover | Self::Input => {
                Some(PositionSpatialModel::SingleAt)
            }
            Self::Scroll => Some(PositionSpatialModel::SingleXy),
            Self::Drag | Self::ModifiedClick => Some(PositionSpatialModel::MultipleXy),
            _ => None,
        }
    }
}

pub fn position_schema_for_family(family: OperationFamily) -> Value {
    match family.position_spatial_model() {
        Some(PositionSpatialModel::SingleAt) => schema_at_route(),
        Some(PositionSpatialModel::SingleXy) => schema_xy_route(),
        Some(PositionSpatialModel::MultipleXy) => schema_positions_route(),
        None => json!({
            "type": "object",
            "additionalProperties": true
        }),
    }
}

fn schema_at_route() -> Value {
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

fn schema_xy_route() -> Value {
    schema_at_route()
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

/// Virtual pipeline tools for the Position LLM (flat parameters per route).
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
        Some(PositionSpatialModel::SingleAt) => vec![pipeline_tool(
            "submit_position_at",
            "Submit session coordinates of the target after spatial proof (Steps 1-4 in thinking).",
            schema_at_route(),
        )],
        Some(PositionSpatialModel::SingleXy) => vec![pipeline_tool(
            "submit_position_xy",
            "Submit pointer anchor (x, y) on the scroll region after spatial proof.",
            schema_xy_route(),
        )],
        Some(PositionSpatialModel::MultipleXy) => vec![pipeline_tool(
            "submit_position_positions",
            "Submit session coordinate list after proving each anchor.",
            schema_positions_route(),
        )],
        None => Vec::new(),
    }
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
        PositionSpatialModel::SingleAt | PositionSpatialModel::SingleXy => {
            if let Some(x) = pos.x {
                obj.insert("x".into(), json!(x));
            }
            if let Some(y) = pos.y {
                obj.insert("y".into(), json!(y));
            }
        }
        PositionSpatialModel::MultipleXy => merge_multiple_xy(family, obj, pos),
    }
}

fn merge_multiple_xy(
    family: OperationFamily,
    obj: &mut serde_json::Map<String, Value>,
    pos: &PositionModuleOutput,
) {
    let positions = collect_positions(pos);
    if positions.is_empty() {
        return;
    }
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

pub fn resolve_execution_tool(decision_name: &str, pos: &PositionModuleOutput) -> String {
    let n = decision_name.trim().to_ascii_lowercase();
    match n.as_str() {
        "click" => "mouse_click_at",
        "double_click" => "mouse_double_click_at",
        "right_click" => "mouse_right_click_at",
        "hover" | "mouse_move" => "move_to",
        "scroll" => "mouse_scroll_current",
        "drag" => "mouse_drag_from_to_at",
        "input" => "input_at",
        "modified_click" => {
            // A range select is two anchors; everything else is a flat list.
            if collect_positions(pos).len() == 2 {
                "modified_click_range_select_at"
            } else {
                "modified_click_select_at"
            }
        }
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::computer::pipeline::types::PositionSubmitRoute;

    #[test]
    fn families_map_to_spatial_models() {
        assert_eq!(
            OperationFamily::PointerClick.position_spatial_model(),
            Some(PositionSpatialModel::SingleAt)
        );
        assert_eq!(
            OperationFamily::Drag.position_spatial_model(),
            Some(PositionSpatialModel::MultipleXy)
        );
        assert_eq!(
            OperationFamily::ModifiedClick.position_spatial_model(),
            Some(PositionSpatialModel::MultipleXy)
        );
        assert_eq!(OperationFamily::Wait.position_spatial_model(), None);
    }

    /// No family may offer an index submit tool — the numbered overlay is gone.
    #[test]
    fn no_family_offers_an_index_submit_tool() {
        for family in [
            OperationFamily::PointerClick,
            OperationFamily::PointerHover,
            OperationFamily::Input,
            OperationFamily::Scroll,
            OperationFamily::Drag,
            OperationFamily::ModifiedClick,
        ] {
            for tool in position_submit_tools(family) {
                let name = tool["function"]["name"].as_str().unwrap_or_default();
                assert!(
                    !name.contains("index"),
                    "{family:?} still offers {name}"
                );
            }
        }
    }

    /// Every schema is coordinates only — no `index`, no `indices`, no `reference_index`.
    #[test]
    fn no_schema_mentions_an_index_field() {
        for family in [
            OperationFamily::PointerClick,
            OperationFamily::PointerHover,
            OperationFamily::Input,
            OperationFamily::Scroll,
            OperationFamily::Drag,
            OperationFamily::ModifiedClick,
        ] {
            let text = position_schema_for_family(family).to_string();
            assert!(!text.contains("\"index\""), "{family:?}: {text}");
            assert!(!text.contains("\"indices\""), "{family:?}: {text}");
            assert!(!text.contains("reference_index"), "{family:?}: {text}");
        }
    }

    #[test]
    fn merge_modified_click_writes_positions() {
        let mut args = json!({"goal": "g"});
        let pos = PositionModuleOutput {
            positions: Some(vec![PositionPoint { x: 10, y: 20 }]),
            ..Default::default()
        };
        merge_position_output(OperationFamily::ModifiedClick, &mut args, &pos);
        assert_eq!(args["positions"], json!([{"x": 10, "y": 20}]));
    }

    #[test]
    fn merge_drag_positions_to_from_to() {
        let mut args = json!({"goal": "g"});
        let pos = PositionModuleOutput {
            positions: Some(vec![
                PositionPoint { x: 3, y: 4 },
                PositionPoint { x: 7, y: 8 },
            ]),
            ..Default::default()
        };
        merge_position_output(OperationFamily::Drag, &mut args, &pos);
        assert_eq!(args["x1"], 3);
        assert_eq!(args["y1"], 4);
        assert_eq!(args["x2"], 7);
        assert_eq!(args["y2"], 8);
    }

    #[test]
    fn resolve_modified_click_range_for_two_positions() {
        let pos = PositionModuleOutput {
            positions: Some(vec![
                PositionPoint { x: 1, y: 2 },
                PositionPoint { x: 5, y: 6 },
            ]),
            ..Default::default()
        };
        assert_eq!(
            resolve_execution_tool("modified_click", &pos),
            "modified_click_range_select_at"
        );
    }

    #[test]
    fn at_route_merges_the_point() {
        let pos = PositionModuleOutput {
            submit_route: Some(PositionSubmitRoute::At),
            x: Some(279),
            y: Some(332),
            ..Default::default()
        };
        assert_eq!(resolve_execution_tool("click", &pos), "mouse_click_at");
        let mut args = json!({"action": "Click copy icon"});
        merge_position_output(OperationFamily::PointerClick, &mut args, &pos);
        assert_eq!(args["x"], 279);
        assert_eq!(args["y"], 332);
    }

    /// `resolve_execution_tool` is a pure name mapping now: no route flags, no
    /// branch that could quietly pick a tool that no longer exists.
    #[test]
    fn every_decision_name_resolves_to_an_existing_tool() {
        let empty = PositionModuleOutput::default();
        let expected = [
            ("click", "mouse_click_at"),
            ("double_click", "mouse_double_click_at"),
            ("right_click", "mouse_right_click_at"),
            ("hover", "move_to"),
            ("mouse_move", "move_to"),
            ("scroll", "mouse_scroll_current"),
            ("drag", "mouse_drag_from_to_at"),
            ("input", "input_at"),
            ("modified_click", "modified_click_select_at"),
        ];
        for (decision, tool) in expected {
            assert_eq!(resolve_execution_tool(decision, &empty), tool);
        }
    }
}
