//! UI display labels and parameter summaries for tool invocations (not sent to the LLM).

use super::registry_tool_base_name;
use crate::i18n::{t, tf, UiLocale};
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

fn format_ask_user_summary(args: &Value) -> String {
    let question = str_field(args, &["question"]).unwrap_or_default();
    let mut lines: Vec<String> = vec![question];
    if let Some(options) = args.get("options").and_then(|o| o.as_array()) {
        for (i, opt) in options.iter().enumerate() {
            let label = opt.get("label").and_then(|l| l.as_str()).unwrap_or("?");
            let desc = opt
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            if desc.is_empty() {
                lines.push(format!("{}. {}", i + 1, label));
            } else {
                lines.push(format!("{}. {} - {}", i + 1, label, desc));
            }
        }
    }
    lines.join("\n")
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

fn file_method_label(locale: UiLocale, method: &str) -> String {
    let key = match method {
        "read" => "tools.fileRead",
        "write" => "tools.fileWrite",
        "edit" => "tools.fileEdit",
        "glob" => "tools.fileGlob",
        "grep" => "tools.fileGrep",
        "list" => "tools.fileList",
        _ => "tools.fileOp",
    };
    t(locale, key)
}

fn mouse_method_label(locale: UiLocale, method: &str) -> String {
    match method {
        "click_at" | "click_index" => t(locale, "tools.click"),
        "double_click_at" | "double_click_index" => t(locale, "tools.doubleClick"),
        "right_click_at" | "right_click_index" => t(locale, "tools.rightClick"),
        "hover_at" | "hover_index" => t(locale, "tools.hover"),
        "drag_from_to_at" | "drag_from_to_index" => t(locale, "tools.drag"),
        "scroll" => t(locale, "tools.scroll"),
        m if m.contains("type_text") => t(locale, "tools.typeText"),
        m => m.to_string(),
    }
}

fn infer_cron_job_action(args: &Value) -> String {
    if let Some(a) = args.get("action").and_then(|v| v.as_str()) {
        let t = a.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let has_create =
        str_field(args, &["prompt_text"]).is_some() && str_field(args, &["schedule"]).is_some();
    if has_create {
        "create".into()
    } else {
        "list".into()
    }
}

fn cron_job_action_label(locale: UiLocale, action: &str) -> String {
    let key = match action {
        "create" => "tools.cronCreate",
        "list" => "tools.cronList",
        "enable" => "tools.cronEnable",
        "disable" => "tools.cronDisable",
        "delete" => "tools.cronDelete",
        _ => "tools.cron",
    };
    t(locale, key)
}

fn cron_job_summary(action: &str, args: &Value) -> String {
    match action {
        "create" => str_field(args, &["label"])
            .or_else(|| str_field(args, &["schedule"]))
            .or_else(|| {
                str_field(args, &["prompt_text"])
                    .map(|s| truncate(s.lines().next().unwrap_or(s.as_str()), SUMMARY_MAX))
            })
            .unwrap_or_default(),
        "list" => String::new(),
        _ => str_field(args, &["label"]).unwrap_or_default(),
    }
}

fn computer_action_summary(locale: UiLocale, args: &Value) -> String {
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
        return tf(locale, "tools.indexN", &[("idx", &idx.to_string())]);
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
    str_field(args, &["keys"])
        .map(|s| truncate(&s, SUMMARY_MAX))
        .unwrap_or_default()
}

fn task_board_method_label(locale: UiLocale, method: &str) -> String {
    let key = match method {
        "patch" | "" => "tools.patch",
        "replace" => "tools.replace",
        "init" => "tools.init",
        "prune" => "tools.prune",
        "finalize" => "tools.finalize",
        "check_deps" => "tools.checkDeps",
        "get" => "tools.get",
        _ => "tools.op",
    };
    t(locale, key)
}

fn task_board_invoke_summary(locale: UiLocale, method: &str, args: &Value) -> String {
    let ml = task_board_method_label(locale, method);
    if method == "check_deps" {
        if let Some(id) = args
            .get("item_id")
            .or_else(|| args.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return tf(
                locale,
                "tools.checkDepsItem",
                &[("method", ml.as_str()), ("id", id)],
            );
        }
        return ml;
    }
    if method == "init" {
        if let Some(goal) = str_field(args, &["goal"]) {
            return truncate(&goal, SUMMARY_MAX);
        }
        return ml;
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
    tf(
        locale,
        "tools.methodRows",
        &[("method", ml.as_str()), ("count", &rows.len().to_string())],
    )
}

/// Default display formatter using the current settings locale.
pub fn default_display(raw_name: &str, args: &Value) -> ToolDisplay {
    default_display_for(raw_name, args, crate::i18n::current_ui_locale())
}

/// Default display formatter for tools without a custom `display_fn`.
pub fn default_display_for(raw_name: &str, args: &Value, locale: UiLocale) -> ToolDisplay {
    let base = registry_tool_base_name(raw_name);
    let method = resolve_method(raw_name, args);

    let (label, summary) = match base {
        "terminal" => {
            let elevated = args
                .get("elevated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let label = if elevated {
                t(locale, "tools.terminalElevated")
            } else {
                t(locale, "tools.terminal")
            };
            (
                label,
                str_field(args, &["label"])
                    .or_else(|| {
                        str_field(args, &["command"])
                            .map(|c| truncate(c.lines().next().unwrap_or(&c), SUMMARY_MAX))
                    })
                    .unwrap_or_default(),
            )
        }
        n if n.starts_with("file_") => {
            let m = if method.is_empty() {
                "read"
            } else {
                method.as_str()
            };
            (file_method_label(locale, m), file_summary(args, m))
        }
        n if n.starts_with("mouse_") => {
            let ml = mouse_method_label(
                locale,
                if method.is_empty() {
                    "click_index"
                } else {
                    &method
                },
            );
            (
                tf(locale, "tools.mouse", &[("action", ml.as_str())]),
                computer_action_summary(locale, args),
            )
        }
        n if n.starts_with("input_") => {
            let ml = mouse_method_label(
                locale,
                if method.is_empty() {
                    "action"
                } else {
                    &method
                },
            );
            (
                tf(locale, "tools.textInput", &[("action", ml.as_str())]),
                computer_action_summary(locale, args),
            )
        }
        n if n.starts_with("modified_click_") => {
            let ml =
                mouse_method_label(locale, if method.is_empty() { "click" } else { &method });
            (
                tf(locale, "tools.modClick", &[("action", ml.as_str())]),
                computer_action_summary(locale, args),
            )
        }
        "hotkey" => (t(locale, "tools.hotkey"), hotkey_summary(args)),
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
            (
                tf(locale, "tools.waitSecs", &[("secs", secs.as_str())]),
                String::new(),
            )
        }
        "clipboard" => {
            let ml = match method.as_str() {
                "read" => t(locale, "tools.read"),
                "write" => t(locale, "tools.write"),
                _ => {
                    if method.is_empty() {
                        t(locale, "tools.op")
                    } else {
                        method.clone()
                    }
                }
            };
            (
                tf(locale, "tools.clipboard", &[("action", ml.as_str())]),
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
                        t(locale, "tools.skillReadResource")
                    } else {
                        t(locale, "tools.skillLoad")
                    }
                }
                "patch" => t(locale, "tools.skillPatch"),
                _ => t(locale, "tools.skill"),
            };
            (
                label,
                str_field(args, &["skill_id", "resource", "path"])
                    .map(|s| truncate(&s, SUMMARY_MAX))
                    .unwrap_or_default(),
            )
        }
        "session_search" => {
            let q = str_field(args, &["query"]).unwrap_or_default();
            (
                t(locale, "tools.sessionSearch"),
                truncate(&q, SUMMARY_MAX),
            )
        }
        "session_read" => {
            let summary = str_field(args, &["around_message_id"])
                .or_else(|| {
                    args.get("offset").and_then(|v| {
                        if let Some(n) = v.as_i64() {
                            Some(tf(locale, "tools.messageN", &[("n", &n.to_string())]))
                        } else if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty())
                        {
                            Some(tf(locale, "tools.messageN", &[("n", s)]))
                        } else {
                            None
                        }
                    })
                })
                .or_else(|| str_field(args, &["conversation_id", "session_id"]))
                .unwrap_or_default();
            (
                t(locale, "tools.sessionRead"),
                truncate(&summary, SUMMARY_MAX),
            )
        }
        "web_search" => {
            let q = str_field(args, &["query"]).unwrap_or_default();
            (t(locale, "tools.webSearch"), truncate(&q, SUMMARY_MAX))
        }
        "web_fetch" => {
            let u = str_field(args, &["url"]).unwrap_or_else(|| {
                args.get("urls")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            });
            (t(locale, "tools.webFetch"), truncate(&u, SUMMARY_MAX))
        }
        "media_understand" => {
            let goal = str_field(args, &["label", "goal", "question"]).unwrap_or_default();
            (
                t(locale, "tools.mediaUnderstand"),
                truncate(&goal, SUMMARY_MAX),
            )
        }
        "run_subagent" => (
            t(locale, "tools.runSubagent"),
            str_field(args, &["title", "goal", "agentId"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .unwrap_or_default(),
        ),
        "read_lints" => (t(locale, "tools.readLints"), file_summary(args, "read")),
        n if n.starts_with("task_board") => {
            let m = if method.is_empty() {
                "patch"
            } else {
                method.as_str()
            };
            let action = task_board_method_label(locale, m);
            (
                tf(locale, "tools.taskBoard", &[("action", action.as_str())]),
                task_board_invoke_summary(locale, m, args),
            )
        }
        "list_apps" => (
            t(locale, "tools.listApps"),
            str_field(args, &["goal"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .unwrap_or_default(),
        ),
        "launch_app" => (
            t(locale, "tools.launchApp"),
            str_field(args, &["app"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .or_else(|| str_field(args, &["goal"]).map(|s| truncate(&s, SUMMARY_MAX)))
                .unwrap_or_default(),
        ),
        "cron_job" => {
            let action = infer_cron_job_action(args);
            (
                cron_job_action_label(locale, &action),
                cron_job_summary(&action, args),
            )
        }
        "job" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("list")
                .trim();
            let key = match action {
                "list" => "tools.jobList",
                "status" => "tools.jobStatus",
                "await" => "tools.jobAwait",
                "cancel" => "tools.jobCancel",
                _ => "tools.job",
            };
            (t(locale, key), String::new())
        }
        "ask_user" => (t(locale, "tools.askUser"), format_ask_user_summary(args)),
        "response" => (t(locale, "tools.response"), String::new()),
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

/// Format display for a tool invocation using the current settings locale.
pub fn format_tool_display(
    raw_name: &str,
    args: &Value,
    custom: Option<&ToolDisplayFn>,
) -> ToolDisplay {
    format_tool_display_for(raw_name, args, custom, crate::i18n::current_ui_locale())
}

/// Format display for a tool invocation.
pub fn format_tool_display_for(
    raw_name: &str,
    args: &Value,
    custom: Option<&ToolDisplayFn>,
    locale: UiLocale,
) -> ToolDisplay {
    if let Some(f) = custom {
        return f(raw_name, args);
    }
    default_display_for(raw_name, args, locale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn zh(raw_name: &str, args: &Value) -> ToolDisplay {
        default_display_for(raw_name, args, UiLocale::ZhCn)
    }

    #[test]
    fn file_read_label_and_basename_only() {
        let d = zh("file_read", &json!({"path": "src/App.vue"}));
        assert_eq!(d.label, "读取文件");
        assert_eq!(d.summary, "App.vue");
        assert!(!d.summary.contains('/'));
    }

    #[test]
    fn file_edit_label_uses_top_level_path() {
        let d = zh(
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
        let d = zh(
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
        let d = zh(
            "file_grep",
            &json!({"pattern": "fn main", "path": "src/components/App.vue"}),
        );
        assert_eq!(d.label, "搜索内容");
        assert_eq!(d.summary, "fn main");
    }

    #[test]
    fn file_glob_shows_pattern() {
        let d = zh("file_glob", &json!({"pattern": "**/*.rs", "base": "src"}));
        assert_eq!(d.label, "搜索文件");
        assert_eq!(d.summary, "**/*.rs");
    }

    #[test]
    fn file_glob_shows_pattern_not_search_root() {
        let d = zh(
            "file_glob",
            &json!({"pattern": "**/*.vue", "path": "src/components", "base": "src"}),
        );
        assert_eq!(d.label, "搜索文件");
        assert_eq!(d.summary, "**/*.vue");
    }

    #[test]
    fn file_grep_keeps_pattern_not_path() {
        let d = zh("file_grep", &json!({"pattern": "fn main"}));
        assert_eq!(d.label, "搜索内容");
        assert_eq!(d.summary, "fn main");
    }

    #[test]
    fn terminal_command_summary() {
        let d = zh("terminal", &json!({"command": "npm test"}));
        assert_eq!(d.label, "终端命令");
        assert_eq!(d.summary, "npm test");
    }

    #[test]
    fn media_understand_summary_uses_goal() {
        let d = zh(
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
    fn media_understand_summary_prefers_label() {
        let d = zh(
            "media_understand",
            &json!({
                "refs": ["pointer-media://c/a.pdf"],
                "label": "识别发票",
                "goal": "总结合同中的违约责任条款"
            }),
        );
        assert_eq!(d.summary, "识别发票");
    }

    #[test]
    fn session_search_label_uses_query() {
        let d = zh("session_search", &json!({"query": "上次改过登录"}));
        assert_eq!(d.label, "搜索会话");
        assert_eq!(d.summary, "上次改过登录");
    }

    #[test]
    fn session_read_label_uses_offset() {
        let d = zh("session_read", &json!({"offset": 12, "limit": 40}));
        assert_eq!(d.label, "读取会话");
        assert_eq!(d.summary, "第 12 条");
    }

    #[test]
    fn web_search_summary_uses_query() {
        let d = zh("web_search", &json!({"query": "Rust 2024 edition"}));
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
        let d = zh(
            "mouse_click_at",
            &json!({"action": "Click Save", "x": 1, "y": 2}),
        );
        assert!(d.label.contains("鼠标"));
        assert!(d.summary.contains("Save"));
    }

    #[test]
    fn launch_app_label_and_app_summary() {
        let d = zh(
            "launch_app",
            &json!({"goal": "打开微信", "app": "WeChat", "action": "启动微信"}),
        );
        assert_eq!(d.label, "启动应用");
        assert_eq!(d.summary, "WeChat");
    }

    #[test]
    fn list_apps_label_and_goal_summary() {
        let d = zh("list_apps", &json!({"goal": "查找微信"}));
        assert_eq!(d.label, "列出应用");
        assert_eq!(d.summary, "查找微信");
    }

    #[test]
    fn cron_job_create_label_and_schedule_summary() {
        let d = zh(
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
        let d = zh(
            "cron_job",
            &json!({"action": "delete", "job_id": "cron-abc123"}),
        );
        assert_eq!(d.label, "删除定时任务");
        assert_eq!(d.summary, "");
    }

    #[test]
    fn cron_job_delete_with_label_summary() {
        let d = zh(
            "cron_job",
            &json!({"action": "delete", "job_id": "cron-abc123", "label": "每分钟提醒"}),
        );
        assert_eq!(d.label, "删除定时任务");
        assert_eq!(d.summary, "每分钟提醒");
    }

    #[test]
    fn terminal_label_takes_priority_over_command() {
        let d = zh(
            "terminal",
            &json!({"command": "cargo test -p pointer-core task_board::", "label": "运行 task_board 单元测试"}),
        );
        assert_eq!(d.label, "终端命令");
        assert_eq!(d.summary, "运行 task_board 单元测试");
    }

    #[test]
    fn terminal_falls_back_to_command_when_label_absent() {
        let d = zh("terminal", &json!({"command": "cargo build --release"}));
        assert_eq!(d.label, "终端命令");
        assert_eq!(d.summary, "cargo build --release");
    }

    #[test]
    fn ask_user_formats_question_and_options_summary() {
        let d = zh(
            "ask_user",
            &json!({
                "question": "是否允许桌面控制？",
                "options": [{"label": "允许"}, {"label": "仅步骤"}]
            }),
        );
        assert_eq!(d.label, "询问用户");
        assert_eq!(d.summary, "是否允许桌面控制？\n1. 允许\n2. 仅步骤");
    }

    #[test]
    fn job_await_uses_wait_label() {
        let d = zh("job", &json!({"action": "await", "mode": "any"}));
        assert_eq!(d.label, "等待后台任务");
        assert!(d.summary.is_empty());
    }

    #[test]
    fn english_labels_for_common_tools() {
        let d = default_display_for("file_read", &json!({"path": "a.rs"}), UiLocale::En);
        assert_eq!(d.label, "Read file");
        let d = default_display_for("terminal", &json!({"command": "ls"}), UiLocale::En);
        assert_eq!(d.label, "Terminal");
    }
}
