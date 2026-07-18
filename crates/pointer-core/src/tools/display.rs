//! UI display labels and parameter summaries for tool invocations (not sent to the LLM).

use super::registry_tool_base_name;
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
    let mut end = max.saturating_sub(1);
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &t[..end])
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
    let base = registry_tool_base_name(raw_name);
    // For flat tools (file_read, task_board_init, etc.), derive method from suffix.
    if let Some(m) = flat_method_from_tool_name(base) {
        return m;
    }
    let from_method = args
        .get("method")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string();
    if !from_method.is_empty() {
        return from_method;
    }
    crate::agents::computer::tools::method_route::operation_name_for_display(base, raw_name, args)
}

/// For flat renamed tools, extract method from the suffix.
/// `file_read` → Some("read"), `task_board_patch` → Some("patch").
fn flat_method_from_tool_name(name: &str) -> Option<String> {
    if let Some(suffix @ ("read" | "write" | "edit" | "glob" | "grep" | "list")) =
        name.strip_prefix("file_")
    {
        return Some(suffix.to_string());
    }
    if let Some(suffix) = name.strip_prefix("mouse_") {
        return Some(suffix.to_string());
    }
    if let Some(suffix) = name.strip_prefix("input_") {
        return Some(suffix.to_string());
    }
    if let Some(suffix) = name.strip_prefix("modified_click_") {
        return Some(suffix.to_string());
    }
    if let Some(suffix) = name.strip_prefix("clipboard_") {
        return Some(suffix.to_string());
    }
    if let Some(suffix) = name.strip_prefix("captcha_verify_") {
        return Some(suffix.to_string());
    }
    if let Some(suffix) = name.strip_prefix("skill_") {
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }
    if let Some(suffix) = name.strip_prefix("task_board_") {
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }
    None
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
        "click_at" | "click_index" => "点击".to_string(),
        "double_click_at" | "double_click_index" => "双击".to_string(),
        "right_click_at" | "right_click_index" => "右键".to_string(),
        "hover_at" | "hover_index" => "悬停".to_string(),
        "drag_from_to_at" | "drag_from_to_index" => "拖拽".to_string(),
        "scroll" => "滚动".to_string(),
        m if m.contains("type_text") => "输入文字".to_string(),
        m => m.to_string(),
    }
}

fn captcha_action_label(action: &str) -> &'static str {
    match action {
        "click" => "点选",
        "drag" => "拖拽",
        "input" => "输入",
        "solve" => "识别",
        _ => "识别",
    }
}

fn infer_cron_job_action(args: &Value) -> String {
    if let Some(a) = args.get("action").and_then(|v| v.as_str()) {
        let t = a.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let has_create = str_field(args, &["prompt_text"]).is_some()
        && str_field(args, &["schedule"]).is_some();
    if has_create {
        "create".into()
    } else {
        "list".into()
    }
}

fn cron_job_action_label(action: &str) -> &'static str {
    match action {
        "create" => "创建定时任务",
        "list" => "列出定时任务",
        "enable" => "启用定时任务",
        "disable" => "停用定时任务",
        "delete" => "删除定时任务",
        _ => "定时任务",
    }
}

fn cron_job_summary(action: &str, args: &Value) -> String {
    match action {
        "create" => str_field(args, &["label"])
            .or_else(|| str_field(args, &["schedule"]))
            .or_else(|| {
                str_field(args, &["prompt_text"]).map(|s| {
                    truncate(s.lines().next().unwrap_or(s.as_str()), SUMMARY_MAX)
                })
            })
            .unwrap_or_default(),
        "list" => String::new(),
        _ => str_field(args, &["label"]).unwrap_or_default(),
    }
}

fn computer_action_summary(args: &Value) -> String {
    fn has_workspace_noise(s: &str) -> bool {
        let lower = s.to_lowercase();
        lower.contains("工作目录") || lower.contains("workspace")
    }

    if let Some(a) = str_field(args, &["action"]) {
        if !has_workspace_noise(&a) {
            return truncate(&a, SUMMARY_MAX);
        }
    }
    if let Some(g) = str_field(args, &["goal"]) {
        if !has_workspace_noise(&g) {
            return truncate(&g, SUMMARY_MAX);
        }
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

fn file_summary(args: &Value, method: &str) -> String {
    // grep / glob: always show search pattern only (never base/path/directory basename).
    if method == "grep" || method == "glob" {
        return str_field(args, &["pattern"])
            .map(|p| truncate(&p, SUMMARY_MAX))
            .unwrap_or_default();
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

fn task_board_method_label(method: &str) -> &'static str {
    match method {
        "patch" | "" => "更新",
        "replace" => "替换",
        "init" => "初始化",
        "prune" => "清理",
        "finalize" => "完成",
        "sync_finding" => "同步发现",
        "check_deps" => "检查依赖",
        "get" => "读取",
        _ => "操作",
    }
}

fn task_board_invoke_summary(method: &str, args: &Value) -> String {
    let ml = task_board_method_label(method);
    if method == "check_deps" {
        if let Some(id) = args
            .get("item_id")
            .or_else(|| args.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return format!("{ml} · #{id}");
        }
        return ml.to_string();
    }
    if method == "init" {
        if let Some(goal) = str_field(args, &["goal"]) {
            return truncate(&goal, SUMMARY_MAX);
        }
        return ml.to_string();
    }
    let rows = crate::task_board::args::board_rows_from_args(args);
    if rows.is_empty() {
        return String::new();
    }
    if rows.len() == 1 {
        let row = &rows[0];
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("?");
        let status = row
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(st) = status {
            return format!("#{id} → {st}");
        }
        return format!("#{id}");
    }
    format!("{ml} · {} 行", rows.len())
}

/// Default display formatter for tools without a custom `display_fn`.
pub fn default_display(raw_name: &str, args: &Value) -> ToolDisplay {
    let base = registry_tool_base_name(raw_name);
    let method = resolve_method(raw_name, args);

    let (label, summary) = match base {
        "terminal" => {
            let elevated = args
                .get("elevated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let label = if elevated {
                "终端命令（提权）".to_string()
            } else {
                "终端命令".to_string()
            };
            (
                label,
                str_field(args, &["label"])
                    .or_else(|| {
                        str_field(args, &["command"]).map(|c| {
                            truncate(c.lines().next().unwrap_or(&c), SUMMARY_MAX)
                        })
                    })
                    .unwrap_or_default(),
            )
        }
        n if n.starts_with("file_") => {
            let m = if method.is_empty() { "read" } else { method.as_str() };
            (file_method_label(m).to_string(), file_summary(args, m))
        }
        n if n.starts_with("mouse_") => {
            let ml = mouse_method_label(if method.is_empty() { "click_index" } else { &method });
            (format!("鼠标 · {ml}"), computer_action_summary(args))
        }
        n if n.starts_with("input_") => {
            let ml = mouse_method_label(if method.is_empty() { "action" } else { &method });
            (format!("文本输入 · {ml}"), computer_action_summary(args))
        }
        n if n.starts_with("modified_click_") => {
            let ml = mouse_method_label(if method.is_empty() { "click" } else { &method });
            (format!("修饰点击 · {ml}"), computer_action_summary(args))
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
        n if n.starts_with("skill_") => {
            let label = match method.as_str() {
                "read" => {
                    let has_path = str_field(args, &["path", "resource"])
                        .is_some_and(|s| !s.trim().is_empty());
                    if has_path {
                        "读取技能资源"
                    } else {
                        "加载技能"
                    }
                }
                "patch" => "更新技能",
                _ => "技能",
            };
            (
                label.to_string(),
                str_field(args, &["skill_id", "resource", "path"])
                    .map(|s| truncate(&s, SUMMARY_MAX))
                    .unwrap_or_default(),
            )
        }
        "web_search" => { let q = str_field(args, &["query"]).unwrap_or_default(); ("联网搜索".to_string(), truncate(&q, SUMMARY_MAX)) }
        "media_understand" => {
            let goal = str_field(args, &["goal", "question"]).unwrap_or_default();
            ("媒体理解".to_string(), truncate(&goal, SUMMARY_MAX))
        }
        "run_subagent" => ("委派子任务".to_string(), str_field(args, &["title", "goal", "agentId"]).map(|s| truncate(&s, SUMMARY_MAX)).unwrap_or_default()),
        "read_lints" => ("代码检查".to_string(), file_summary(args, "read")),
        n if n.starts_with("task_board") => {
            let m = if method.is_empty() { "patch" } else { method.as_str() };
            (format!("任务板 · {}", task_board_method_label(m)), task_board_invoke_summary(m, args))
        }
        "captcha_verify" => {
            let action = if method.is_empty() { str_field(args, &["action", "method"]).unwrap_or_default() } else { method.clone() };
            (format!("验证码 · {}", captcha_action_label(action.as_str())), computer_action_summary(args))
        }
        "list_apps" => (
            "列出应用".to_string(),
            str_field(args, &["goal"]).map(|s| truncate(&s, SUMMARY_MAX)).unwrap_or_default(),
        ),
        "launch_app" => (
            "启动应用".to_string(),
            str_field(args, &["app"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .or_else(|| str_field(args, &["goal"]).map(|s| truncate(&s, SUMMARY_MAX)))
                .unwrap_or_default(),
        ),
        "cron_job" => {
            let action = infer_cron_job_action(args);
            (
                cron_job_action_label(&action).to_string(),
                cron_job_summary(&action, args),
            )
        }
        "response" => ("回复用户".to_string(), String::new()),
        _ => if !method.is_empty() { (format!("{base} · {method}"), String::new()) } else { (raw_name.to_string(), String::new()) }
    };

    ToolDisplay { label, summary }
}

/// Format display for a tool invocation.
pub fn format_tool_display(
    raw_name: &str,
    args: &Value,
    custom: Option<&ToolDisplayFn>,
) -> ToolDisplay {
    if let Some(f) = custom {
        return f(raw_name, args);
    }
    default_display(raw_name, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_read_label_and_basename_only() {
        let d = default_display("file_read", &json!({"path": "src/App.vue"}));
        assert_eq!(d.label, "读取文件");
        assert_eq!(d.summary, "App.vue");
        assert!(!d.summary.contains('/'));
    }

    #[test]
    fn file_edit_label_uses_top_level_path() {
        let d = default_display(
            "file_edit",
            &json!({
                "path": "src/App.vue",
                "oldString": "a",
                "newString": "b"
            }),
        );
        assert_eq!(d.label, "编辑文件");
        assert_eq!(d.summary, "App.vue");
    }

    #[test]
    fn task_board_flat_patch_shows_item_and_status() {
        let d = default_display(
            "task_board_patch",
            &json!({
                "item_id": "2",
                "status": "done",
                "validate_results": "窗口已打开"
            }),
        );
        assert_eq!(d.label, "任务板 · 更新");
        assert_eq!(d.summary, "#2 → done");
    }

    #[test]
    fn file_grep_shows_pattern_not_search_path() {
        let d = default_display(
            "file_grep",
            &json!({"pattern": "fn main", "path": "src/components/App.vue"}),
        );
        assert_eq!(d.label, "搜索内容");
        assert_eq!(d.summary, "fn main");
    }

    #[test]
    fn file_glob_shows_pattern() {
        let d = default_display("file_glob", &json!({"pattern": "**/*.rs", "base": "src"}));
        assert_eq!(d.label, "搜索文件");
        assert_eq!(d.summary, "**/*.rs");
    }

    #[test]
    fn file_glob_shows_pattern_not_search_root() {
        let d = default_display(
            "file_glob",
            &json!({"pattern": "**/*.vue", "path": "src/components", "base": "src"}),
        );
        assert_eq!(d.label, "搜索文件");
        assert_eq!(d.summary, "**/*.vue");
    }

    #[test]
    fn file_grep_keeps_pattern_not_path() {
        let d = default_display("file_grep", &json!({"pattern": "fn main"}));
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
    fn media_understand_summary_uses_goal() {
        let d = default_display(
            "media_understand",
            &json!({
                "refs": ["pointer-media://c/a.pdf"],
                "mode": "pdf",
                "goal": "总结合同中的违约责任条款"
            }),
        );
        assert_eq!(d.label, "媒体理解");
        assert_eq!(d.summary, "总结合同中的违约责任条款");
    }

    #[test]
    fn web_search_summary_uses_query() {
        let d = default_display("web_search", &json!({"query": "Rust 2024 edition"}));
        assert_eq!(d.label, "联网搜索");
        assert_eq!(d.summary, "Rust 2024 edition");
    }

    #[test]
    fn truncate_does_not_split_utf8_codepoint() {
        let s = "阿里云 Qwen3.7 Max Preview Plus 2026 年 5 月 发布详情";
        let out = truncate(s, 56);
        assert!(out.ends_with('…'));
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn mouse_prefers_action() {
        let d = default_display(
            "mouse_click_at",
            &json!({"action": "Click Save", "x": 1, "y": 2}),
        );
        assert!(d.label.contains("鼠标"));
        assert!(d.summary.contains("Save"));
    }

    #[test]
    fn launch_app_label_and_app_summary() {
        let d = default_display(
            "launch_app",
            &json!({"goal": "打开微信", "app": "WeChat", "action": "启动微信"}),
        );
        assert_eq!(d.label, "启动应用");
        assert_eq!(d.summary, "WeChat");
    }

    #[test]
    fn list_apps_label_and_goal_summary() {
        let d = default_display("list_apps", &json!({"goal": "查找微信"}));
        assert_eq!(d.label, "列出应用");
        assert_eq!(d.summary, "查找微信");
    }

    #[test]
    fn cron_job_create_label_and_schedule_summary() {
        let d = default_display(
            "cron_job",
            &json!({
                "action": "create",
                "prompt_text": "每天检查邮件",
                "schedule": "daily@9:30"
            }),
        );
        assert_eq!(d.label, "创建定时任务");
        assert_eq!(d.summary, "daily@9:30");
    }

    #[test]
    fn cron_job_delete_uses_job_id_summary() {
        let d = default_display(
            "cron_job",
            &json!({"action": "delete", "job_id": "cron-abc123"}),
        );
        assert_eq!(d.label, "删除定时任务");
        assert_eq!(d.summary, "");
    }

    #[test]
    fn cron_job_delete_with_label_summary() {
        let d = default_display(
            "cron_job",
            &json!({"action": "delete", "job_id": "cron-abc123", "label": "每分钟提醒"}),
        );
        assert_eq!(d.label, "删除定时任务");
        assert_eq!(d.summary, "每分钟提醒");
    }

    #[test]
    fn terminal_label_takes_priority_over_command() {
        let d = default_display(
            "terminal",
            &json!({"command": "cargo test -p pointer-core task_board::", "label": "运行 task_board 单元测试"}),
        );
        assert_eq!(d.label, "终端命令");
        assert_eq!(d.summary, "运行 task_board 单元测试");
    }

    #[test]
    fn terminal_falls_back_to_command_when_label_absent() {
        let d = default_display(
            "terminal",
            &json!({"command": "cargo build --release"}),
        );
        assert_eq!(d.label, "终端命令");
        assert_eq!(d.summary, "cargo build --release");
    }
}
