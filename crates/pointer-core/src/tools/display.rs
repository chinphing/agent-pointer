//! UI display labels and parameter summaries for tool invocations (not sent to the LLM).

use super::{merge_tool_method_from_qualified_name, registry_tool_base_name};
use serde_json::Value;
use std::sync::Arc;

pub struct ToolDisplay {
    pub label: String,
    pub summary: String,
}

pub type ToolDisplayFn = Arc<dyn Fn(&str, &Value) -> ToolDisplay + Send + Sync>;

const SUMMARY_MAX: usize = 56;

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.len() <= max {
        return t.to_string();
    }
    format!("{}…", &t[..max.saturating_sub(1)])
}

fn str_field(args: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = args.get(*k).and_then(|x| x.as_str()) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn resolve_method(raw_name: &str, args: &Value) -> String {
    if let Some((_, m)) = raw_name.split_once(':') {
        let m = m.trim();
        if !m.is_empty() {
            return m.to_string();
        }
    }
    args.get("method")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn file_method_label(method: &str) -> &'static str {
    match method {
        "read" => "读取文件",
        "write" => "写入文件",
        "edit" => "编辑文件",
        "glob" => "搜索文件",
        "grep" => "搜索内容",
        "list" => "列出目录",
        _ => "文件操作",
    }
}

fn mouse_method_label(method: &str) -> String {
    match method {
        "click_at" | "click_index" | "click_current" => "点击".to_string(),
        "double_click_at" | "double_click_index" | "double_click_current" => "双击".to_string(),
        "right_click_at" | "right_click_index" | "right_click_current" => "右键".to_string(),
        "hover_at" | "hover_index" => "悬停".to_string(),
        "move_offset" => "移动".to_string(),
        "drag_from_to_at" | "drag_from_to_index" => "拖拽".to_string(),
        "scroll_at_current" => "滚动".to_string(),
        m if m.contains("type_text") => "输入文字".to_string(),
        m => m.to_string(),
    }
}

fn computer_action_summary(args: &Value) -> String {
    if let Some(a) = str_field(args, &["action"]) {
        return truncate(&a, SUMMARY_MAX);
    }
    if let Some(g) = str_field(args, &["goal"]) {
        return truncate(&g, SUMMARY_MAX);
    }
    if let (Some(x), Some(y)) = (args.get("x"), args.get("y")) {
        return format!("({}, {})", x, y);
    }
    if let Some(idx) = args.get("index").or_else(|| args.get("indices")) {
        return format!("索引 {idx}");
    }
    String::new()
}

fn path_basename(p: &str) -> String {
    let p = p.trim();
    if p.is_empty() {
        return String::new();
    }
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

fn path_from_value(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Some(obj) = v.as_object() {
        for key in ["path", "file"] {
            if let Some(s) = obj.get(key).and_then(|x| x.as_str()) {
                let t = s.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn join_display_names(names: Vec<String>, max_items: usize) -> String {
    if names.is_empty() {
        return String::new();
    }
    let joined = names.into_iter().take(max_items).collect::<Vec<_>>().join(", ");
    truncate(&joined, SUMMARY_MAX)
}

fn file_summary(args: &Value, method: &str) -> String {
    if let Some(paths) = args.get("paths").and_then(|v| v.as_array()) {
        let names: Vec<String> = paths
            .iter()
            .filter_map(path_from_value)
            .map(|p| path_basename(&p))
            .filter(|s| !s.is_empty())
            .collect();
        if !names.is_empty() {
            return join_display_names(names, 8);
        }
    }

    if method == "edit" {
        if let Some(edits) = args.get("edits").and_then(|v| v.as_array()) {
            let names: Vec<String> = edits
                .iter()
                .filter_map(path_from_value)
                .map(|p| path_basename(&p))
                .filter(|s| !s.is_empty())
                .collect();
            if !names.is_empty() {
                return join_display_names(names, 8);
            }
        }
    }

    if let Some(p) = str_field(args, &["path", "file", "directory"]) {
        return path_basename(&p);
    }

    if let Some(p) = str_field(args, &["pattern"]) {
        return truncate(&p, SUMMARY_MAX);
    }

    String::new()
}

fn hotkey_summary(args: &Value) -> String {
    if let Some(keys) = args.get("keys").and_then(|v| v.as_array()) {
        let parts: Vec<String> = keys
            .iter()
            .filter_map(|k| k.as_str().map(str::trim).filter(|s| !s.is_empty()))
            .map(|s| s.to_string())
            .collect();
        if !parts.is_empty() {
            return truncate(&parts.join("+"), SUMMARY_MAX);
        }
    }
    str_field(args, &["keys"]).map(|s| truncate(&s, SUMMARY_MAX)).unwrap_or_default()
}

/// Default display formatter for tools without a custom `display_fn`.
pub fn default_display(raw_name: &str, args: &Value) -> ToolDisplay {
    let base = registry_tool_base_name(raw_name);
    let method = resolve_method(raw_name, args);

    let (label, summary) = match base {
        "terminal" => (
            "终端命令".to_string(),
            str_field(args, &["command"])
                .map(|c| truncate(c.lines().next().unwrap_or(&c), SUMMARY_MAX))
                .unwrap_or_default(),
        ),
        "file" => {
            let m = if method.is_empty() { "read" } else { method.as_str() };
            (file_method_label(m).to_string(), file_summary(args, m))
        }
        "mouse" => {
            let ml = mouse_method_label(if method.is_empty() { "action" } else { &method });
            (
                format!("鼠标 · {ml}"),
                computer_action_summary(args),
            )
        }
        "composite_action" => {
            let ml = mouse_method_label(if method.is_empty() { "action" } else { &method });
            (
                format!("组合操作 · {ml}"),
                computer_action_summary(args),
            )
        }
        "modified_click" => {
            let ml = mouse_method_label(if method.is_empty() { "click" } else { &method });
            (
                format!("修饰点击 · {ml}"),
                computer_action_summary(args),
            )
        }
        "hotkey" => ("快捷键".to_string(), hotkey_summary(args)),
        "wait" => {
            let secs = args
                .get("seconds")
                .or_else(|| args.get("duration"))
                .map(|v| {
                    if let Some(n) = v.as_f64() {
                        n.to_string()
                    } else if let Some(n) = v.as_u64() {
                        n.to_string()
                    } else if let Some(s) = v.as_str() {
                        s.trim().to_string()
                    } else {
                        "?".to_string()
                    }
                })
                .unwrap_or_else(|| "?".to_string());
            (format!("等待 {secs} 秒"), String::new())
        }
        "clipboard" => {
            let ml = match method.as_str() {
                "read" => "读取",
                "write" => "写入",
                _ => if method.is_empty() { "操作" } else { method.as_str() },
            };
            (
                format!("剪贴板 · {ml}"),
                str_field(args, &["text", "content"])
                    .map(|t| truncate(&t, SUMMARY_MAX))
                    .unwrap_or_default(),
            )
        }
        "skill" => {
            let label = match method.as_str() {
                "load_instructions" => "加载技能",
                "read_resource" => "读取技能资源",
                _ => "技能",
            };
            (
                label.to_string(),
                str_field(args, &["skill_id", "resource", "path"])
                    .map(|s| truncate(&s, SUMMARY_MAX))
                    .unwrap_or_default(),
            )
        }
        "run_subagent" => (
            "委派子任务".to_string(),
            str_field(args, &["title", "agentId"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .unwrap_or_default(),
        ),
        "read_lints" => (
            "代码检查".to_string(),
            file_summary(args, "read"),
        ),
        "task_board" => {
            let ml = if method.is_empty() { "patch" } else { method.as_str() };
            (format!("任务板 · {ml}"), String::new())
        }
        "response" => ("回复用户".to_string(), String::new()),
        _ => {
            if !method.is_empty() {
                (format!("{base} · {method}"), String::new())
            } else {
                (raw_name.to_string(), String::new())
            }
        }
    };

    ToolDisplay { label, summary }
}

/// Format display for invocation; merges qualified name into args when needed.
pub fn format_tool_display(
    raw_name: &str,
    args: &Value,
    custom: Option<&ToolDisplayFn>,
) -> ToolDisplay {
    let (_tool_id, merged_args) = merge_tool_method_from_qualified_name(raw_name, args.clone());
    if let Some(f) = custom {
        return f(raw_name, &merged_args);
    }
    default_display(raw_name, &merged_args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_read_label_and_basename_only() {
        let d = default_display("file:read", &json!({"path": "src/App.vue"}));
        assert_eq!(d.label, "读取文件");
        assert_eq!(d.summary, "App.vue");
        assert!(!d.summary.contains('/'));
    }

    #[test]
    fn file_batch_read_basenames_comma_separated() {
        let d = default_display(
            "file:read",
            &json!({
                "paths": [
                    {"path": "/workspace/src/App.vue"},
                    {"path": "/workspace/src/main.ts"}
                ]
            }),
        );
        assert_eq!(d.summary, "App.vue, main.ts");
    }

    #[test]
    fn file_grep_keeps_pattern_not_path() {
        let d = default_display("file:grep", &json!({"pattern": "fn main"}));
        assert_eq!(d.label, "搜索内容");
        assert_eq!(d.summary, "fn main");
    }

    #[test]
    fn terminal_command_summary() {
        let d = default_display("terminal", &json!({"command": "npm test"}));
        assert_eq!(d.label, "终端命令");
        assert_eq!(d.summary, "npm test");
    }

    #[test]
    fn mouse_prefers_action() {
        let d = default_display(
            "mouse:click_at",
            &json!({"action": "Click Save", "x": 1, "y": 2}),
        );
        assert!(d.label.contains("鼠标"));
        assert!(d.summary.contains("Save"));
    }
}
