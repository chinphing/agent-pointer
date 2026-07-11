//! JSON Schema definitions for verify module output.

use serde_json::{json, Value};

pub fn verify_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action_result": {
                "type": "string",
                "enum": ["pass", "fail", "pending", "n/a"]
            },
            "failure_cause": {
                "type": "string",
                "enum": ["wrong_operation", "precision_miss"]
            },
            "step_summary": { "type": "string" },
            "loading_detected": { "type": "boolean" }
        },
        "required": ["action_result", "loading_detected"],
        "additionalProperties": false
    })
}
