//! JSON Schema definitions for verify module output.

use serde_json::{json, Value};

use crate::agents::computer::pipeline::operation::OperationFamily;

/// Verify output schema for one operation family.
///
/// The scroll measurement fields are **only** present for `Scroll`: a click
/// verify that can see `scroll_moved` in its tool schema starts describing
/// 「content moved ~0.3 target-heights」 for a click, which then leaks into the
/// planner as a bogus scroll measurement.
pub fn verify_response_schema(family: OperationFamily) -> Value {
    let mut properties = json!({
        "action_result": {
            "type": "string",
            "enum": ["pass", "fail", "pending"],
            "description": "Whether the action produced the effect `goal` calls for. A difference that is not that effect is not a pass."
        },
        "failure_cause": {
            "type": "string",
            "enum": ["wrong_operation", "precision_miss"]
        },
        "step_summary": {
            "type": "string",
            "description": "One line `<subject>: <before> → <after>` naming the expected effect and what the frames show for it."
        },
        "loading_detected": {
            "type": "boolean",
            "description": "True when a spinner, progress bar, or partial load blocks a firm pass."
        }
    });
    if family == OperationFamily::Scroll {
        // `at_edge` only exists for scroll — a click that fails has no edge.
        properties["failure_cause"]["enum"] = json!(["wrong_operation", "precision_miss", "at_edge"]);
        properties["scroll_moved"] = json!({
            "type": "number",
            "description": "How far the content moved, as a fraction of the target area (0.05 = 5%). Above 1 when the jump exceeded a full panel. Signed by the direction the content actually moved relative to the request."
        });
        properties["scroll_unit"] = json!({
            "type": "string",
            "enum": ["target-heights", "target-widths", "screen", "screen-widths"],
            "description": "The unit of scroll_moved — same words you use in step_summary."
        });
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": ["action_result", "loading_detected"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 滚动字段只给 scroll 场景：点击的 verify 若能在 schema 里看到
    /// `scroll_moved`，就会给点击编一个「内容上移 0.3」。
    #[test]
    fn scroll_fields_only_exist_for_the_scroll_family() {
        let scroll = verify_response_schema(OperationFamily::Scroll);
        assert!(scroll["properties"].get("scroll_moved").is_some(), "{scroll}");
        assert!(scroll["properties"].get("scroll_unit").is_some(), "{scroll}");
        let causes = scroll["properties"]["failure_cause"]["enum"]
            .as_array()
            .expect("failure_cause enum");
        assert!(causes.iter().any(|c| c == "at_edge"), "{scroll}");

        for family in [
            OperationFamily::PointerClick,
            OperationFamily::Input,
            OperationFamily::ModifiedClick,
            OperationFamily::Hotkey,
            OperationFamily::Drag,
            OperationFamily::Clipboard,
        ] {
            let schema = verify_response_schema(family);
            assert!(
                schema["properties"].get("scroll_moved").is_none(),
                "{family:?} 不该带滚动字段: {schema}"
            );
            assert!(
                schema["properties"].get("scroll_unit").is_none(),
                "{family:?} 不该带滚动字段: {schema}"
            );
            // `at_edge` 只属于 scroll：点击没有"边缘"这回事。
            let causes = schema["properties"]["failure_cause"]["enum"]
                .as_array()
                .expect("failure_cause enum");
            assert!(
                !causes.iter().any(|c| c == "at_edge"),
                "{family:?} 不该有 at_edge: {schema}"
            );
            // 其余字段一个都不能少，否则 verify 会解析失败。
            for key in [
                "action_result",
                "failure_cause",
                "step_summary",
                "loading_detected",
            ] {
                assert!(schema["properties"].get(key).is_some(), "{family:?} 缺 {key}");
            }
        }
    }
}
