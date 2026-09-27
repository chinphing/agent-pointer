//! UI display labels and parameter summaries for tool invocations (not sent to the LLM).

use super::registry_tool_base_name;
use crate::i18n::{self, UiLocale};
use serde_json::Value;
use std::sync::Arc;

pub struct ToolDisplay {
    pub label: String,
    pub summary: String,
}

pub type ToolDisplayFn = Arc<dyn Fn(&str, &Value) -> ToolDisplay + Send + Sync>;

const SUMMARY_MAX: usize = 56;

fn locale() -> UiLocale {
    i18n::current_ui_locale()
}

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

fn file_method_label(method: &str, loc: UiLocale) -> &'static str {
    let key = match method {
        "read" => "tool.file.read",
        "write" => "tool.file.write",
        "edit" => "tool.file.edit",
        "glob" => "tool.file.glob",
        "grep" => "tool.file.grep",
        "list" => "tool.file.list",
        _ => "tool.file.generic",
    };
    i18n::t(key, loc)
}

fn mouse_method_label(method: &str, loc: UiLocale) -> String {
    let key = match method {
        "click_at" | "click_index" => Some("tool.mouse.click"),
        "double_click_at" | "double_click_index" => Some("tool.mouse.double_click"),
        "right_click_at" | "right_click_index" => Some("tool.mouse.right_click"),
        "hover_at" | "hover_index" => Some("tool.mouse.hover"),
        "drag_from_to_at" | "drag_from_to_index" => Some("tool.mouse.drag"),
        "scroll" => Some("tool.mouse.scroll"),
        m if m.contains("type_text") => Some("tool.mouse.type_text"),
        _ => None,
    };
    match key {
        Some(k) => i18n::t(k, loc).to_string(),
        None => method.to_string(),
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

fn cron_job_action_label(action: &str, loc: UiLocale) -> &'static str {
    let key = match action {
        "create" => "tool.cron_job.create",
        "list" => "tool.cron_job.list",
        "enable" => "tool.cron_job.enable",
        "disable" => "tool.cron_job.disable",
        "delete" => "tool.cron_job.delete",
        _ => "tool.cron_job.generic",
    };
    i18n::t(key, loc)
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

fn computer_action_summary(args: &Value, loc: UiLocale) -> String {
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
        return i18n::tf("tool.index", loc, &[("idx", &idx.to_string())]);
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

fn task_board_method_label(method: &str, loc: UiLocale) -> &'static str {
    let key = match method {
        "patch" | "" => "tool.task_board.patch",
        "replace" => "tool.task_board.replace",
        "init" => "tool.task_board.init",
        "prune" => "tool.task_board.prune",
        "finalize" => "tool.task_board.finalize",
        "check_deps" => "tool.task_board.check_deps",
        "get" => "tool.task_board.get",
        _ => "tool.task_board.generic",
    };
    i18n::t(key, loc)
}

fn task_board_invoke_summary(method: &str, args: &Value, loc: UiLocale) -> String {
    let ml = task_board_method_label(method, loc);
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
    i18n::tf(
        "tool.task_board.rows",
        loc,
        &[("label", ml), ("count", &rows.len().to_string())],
    )
}

/// Default display formatter for tools without a custom `display_fn`.
pub fn default_display(raw_name: &str, args: &Value) -> ToolDisplay {
    let loc = locale();
    let base = registry_tool_base_name(raw_name);
    let method = resolve_method(raw_name, args);

    let (label, summary) = match base {
        "terminal" => {
            let elevated = args
                .get("elevated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let label = if elevated {
                i18n::t("tool.terminal.label_elevated", loc).to_string()
            } else {
                i18n::t("tool.terminal.label", loc).to_string()
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
            (
                file_method_label(m, loc).to_string(),
                file_summary(args, m),
            )
        }
        n if n.starts_with("mouse_") => {
            let ml = mouse_method_label(
                if method.is_empty() {
                    "click_index"
                } else {
                    &method
                },
                loc,
            );
            (
                format!("{} · {}", i18n::t("tool.mouse.prefix", loc), ml),
                computer_action_summary(args, loc),
            )
        }
        n if n.starts_with("input_") => {
            let ml = mouse_method_label(if method.is_empty() { "action" } else { &method }, loc);
            (
                format!("{} · {}", i18n::t("tool.input.prefix", loc), ml),
                computer_action_summary(args, loc),
            )
        }
        n if n.starts_with("modified_click_") => {
            let ml = mouse_method_label(if method.is_empty() { "click" } else { &method }, loc);
            (
                format!("{} · {}", i18n::t("tool.modified_click.prefix", loc), ml),
                computer_action_summary(args, loc),
            )
        }
        "hotkey" => (
            i18n::t("tool.hotkey.label", loc).to_string(),
            hotkey_summary(args),
        ),
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
                i18n::tf("tool.wait.label", loc, &[("secs", &secs)]),
                String::new(),
            )
        }
        "clipboard" => {
            let ml = match method.as_str() {
                "read" => i18n::t("tool.clipboard.read", loc),
                "write" => i18n::t("tool.clipboard.write", loc),
                _ => {
                    if method.is_empty() {
                        i18n::t("tool.clipboard.op", loc)
                    } else {
                        // Fall back to raw method name for unknown actions.
                        // Leak once so the prefix formatter can borrow a 'static-ish slice —
                        // here we just allocate into the format below.
                        ""
                    }
                }
            };
            let action = if ml.is_empty() {
                method.as_str()
            } else {
                ml
            };
            (
                format!("{} · {}", i18n::t("tool.clipboard.prefix", loc), action),
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
                        i18n::t("tool.skill.read_resource", loc)
                    } else {
                        i18n::t("tool.skill.load", loc)
                    }
                }
                "patch" => i18n::t("tool.skill.update", loc),
                _ => i18n::t("tool.skill.generic", loc),
            };
            (
                label.to_string(),
                str_field(args, &["skill_id", "resource", "path"])
                    .map(|s| truncate(&s, SUMMARY_MAX))
                    .unwrap_or_default(),
            )
        }
        "session_search" => {
            let q = str_field(args, &["query"]).unwrap_or_default();
            (
                i18n::t("tool.session_search.label", loc).to_string(),
                truncate(&q, SUMMARY_MAX),
            )
        }
        "session_read" => {
            let summary = str_field(args, &["around_message_id"])
                .or_else(|| {
                    args.get("offset").and_then(|v| {
                        if let Some(n) = v.as_i64() {
                            Some(i18n::tf(
                                "tool.session_read.offset",
                                loc,
                                &[("n", &n.to_string())],
                            ))
                        } else if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty())
                        {
                            Some(i18n::tf("tool.session_read.offset", loc, &[("n", s)]))
                        } else {
                            None
                        }
                    })
                })
                .or_else(|| str_field(args, &["conversation_id", "session_id"]))
                .unwrap_or_default();
            (
                i18n::t("tool.session_read.label", loc).to_string(),
                truncate(&summary, SUMMARY_MAX),
            )
        }
        "web_search" => {
            let q = str_field(args, &["query"]).unwrap_or_default();
            (
                i18n::t("tool.web_search.label", loc).to_string(),
                truncate(&q, SUMMARY_MAX),
            )
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
            (
                i18n::t("tool.web_fetch.label", loc).to_string(),
                truncate(&u, SUMMARY_MAX),
            )
        }
        "media_understand" => {
            let goal = str_field(args, &["label", "goal", "question"]).unwrap_or_default();
            (
                i18n::t("tool.media_understand.label", loc).to_string(),
                truncate(&goal, SUMMARY_MAX),
            )
        }
        "run_subagent" => (
            i18n::t("tool.run_subagent.label", loc).to_string(),
            str_field(args, &["title", "goal", "agentId"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .unwrap_or_default(),
        ),
        "read_lints" => (
            i18n::t("tool.read_lints.label", loc).to_string(),
            file_summary(args, "read"),
        ),
        n if n.starts_with("task_board") => {
            let m = if method.is_empty() {
                "patch"
            } else {
                method.as_str()
            };
            (
                format!(
                    "{} · {}",
                    i18n::t("tool.task_board.prefix", loc),
                    task_board_method_label(m, loc)
                ),
                task_board_invoke_summary(m, args, loc),
            )
        }
        "list_apps" => (
            i18n::t("tool.list_apps.label", loc).to_string(),
            str_field(args, &["goal"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .unwrap_or_default(),
        ),
        "launch_app" => (
            i18n::t("tool.launch_app.label", loc).to_string(),
            str_field(args, &["app"])
                .map(|s| truncate(&s, SUMMARY_MAX))
                .or_else(|| str_field(args, &["goal"]).map(|s| truncate(&s, SUMMARY_MAX)))
                .unwrap_or_default(),
        ),
        "cron_job" => {
            let action = infer_cron_job_action(args);
            (
                cron_job_action_label(&action, loc).to_string(),
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
                "list" => "tool.job.list",
                "status" => "tool.job.status",
                "await" => "tool.job.await",
                "cancel" => "tool.job.cancel",
                _ => "tool.job.generic",
            };
            (i18n::t(key, loc).to_string(), String::new())
        }
        "ask_user" => (
            i18n::t("tool.ask_user.label", loc).to_string(),
            format_ask_user_summary(args),
        ),
        "response" => (i18n::t("tool.response.label", loc).to_string(), String::new()),
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
    use crate::i18n::ENV_LOCALE_TEST_LOCK;
    use serde_json::json;

    fn with_zh_cn_locale<R>(f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCALE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_lc = std::env::var("LC_ALL").ok();
        let prev_lang = std::env::var("LANG").ok();
        std::env::set_var("LC_ALL", "zh_CN.UTF-8");
        std::env::set_var("LANG", "zh_CN.UTF-8");
        let result = f();
        match prev_lc {
            Some(v) => std::env::set_var("LC_ALL", v),
            None => std::env::remove_var("LC_ALL"),
        }
        match prev_lang {
            Some(v) => std::env::set_var("LANG", v),
            None => std::env::remove_var("LANG"),
        }
        result
    }

    fn with_en_locale<R>(f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCALE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_lc = std::env::var("LC_ALL").ok();
        let prev_lang = std::env::var("LANG").ok();
        std::env::set_var("LC_ALL", "en_US.UTF-8");
        std::env::set_var("LANG", "en_US.UTF-8");
        let result = f();
        match prev_lc {
            Some(v) => std::env::set_var("LC_ALL", v),
            None => std::env::remove_var("LC_ALL"),
        }
        match prev_lang {
            Some(v) => std::env::set_var("LANG", v),
            None => std::env::remove_var("LANG"),
        }
        result
    }

    #[test]
    fn file_read_label_and_basename_only() {
        with_zh_cn_locale(|| {
            let d = default_display("file_read", &json!({"path": "src/App.vue"}));
            assert_eq!(d.label, "读取文件");
            assert_eq!(d.summary, "App.vue");
            assert!(!d.summary.contains('/'));
        });
    }

    #[test]
    fn file_edit_label_uses_top_level_path() {
        with_zh_cn_locale(|| {
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
        });
    }

    #[test]
    fn task_board_flat_patch_shows_item_and_status() {
        with_zh_cn_locale(|| {
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
        });
    }

    #[test]
    fn file_grep_shows_pattern_not_search_path() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "file_grep",
                &json!({"pattern": "fn main", "path": "src/components/App.vue"}),
            );
            assert_eq!(d.label, "搜索内容");
            assert_eq!(d.summary, "fn main");
        });
    }

    #[test]
    fn file_glob_shows_pattern() {
        with_zh_cn_locale(|| {
            let d = default_display("file_glob", &json!({"pattern": "**/*.rs", "base": "src"}));
            assert_eq!(d.label, "搜索文件");
            assert_eq!(d.summary, "**/*.rs");
        });
    }

    #[test]
    fn file_glob_shows_pattern_not_search_root() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "file_glob",
                &json!({"pattern": "**/*.vue", "path": "src/components", "base": "src"}),
            );
            assert_eq!(d.label, "搜索文件");
            assert_eq!(d.summary, "**/*.vue");
        });
    }

    #[test]
    fn file_grep_keeps_pattern_not_path() {
        with_zh_cn_locale(|| {
            let d = default_display("file_grep", &json!({"pattern": "fn main"}));
            assert_eq!(d.label, "搜索内容");
            assert_eq!(d.summary, "fn main");
        });
    }

    #[test]
    fn terminal_command_summary() {
        with_zh_cn_locale(|| {
            let d = default_display("terminal", &json!({"command": "npm test"}));
            assert_eq!(d.label, "终端命令");
            assert_eq!(d.summary, "npm test");
        });
    }

    #[test]
    fn media_understand_summary_uses_goal() {
        with_zh_cn_locale(|| {
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
        });
    }

    #[test]
    fn media_understand_summary_prefers_label() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "media_understand",
                &json!({
                    "refs": ["pointer-media://c/a.pdf"],
                    "label": "识别发票",
                    "goal": "总结合同中的违约责任条款"
                }),
            );
            assert_eq!(d.summary, "识别发票");
        });
    }

    #[test]
    fn session_search_label_uses_query() {
        with_zh_cn_locale(|| {
            let d = default_display("session_search", &json!({"query": "上次改过登录"}));
            assert_eq!(d.label, "搜索会话");
            assert_eq!(d.summary, "上次改过登录");
        });
    }

    #[test]
    fn session_read_label_uses_offset() {
        with_zh_cn_locale(|| {
            let d = default_display("session_read", &json!({"offset": 12, "limit": 40}));
            assert_eq!(d.label, "读取会话");
            assert_eq!(d.summary, "第 12 条");
        });
    }

    #[test]
    fn web_search_summary_uses_query() {
        with_zh_cn_locale(|| {
            let d = default_display("web_search", &json!({"query": "Rust 2024 edition"}));
            assert_eq!(d.label, "联网搜索");
            assert_eq!(d.summary, "Rust 2024 edition");
        });
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
        with_zh_cn_locale(|| {
            let d = default_display(
                "mouse_click_at",
                &json!({"action": "Click Save", "x": 1, "y": 2}),
            );
            assert!(d.label.contains("鼠标"));
            assert!(d.summary.contains("Save"));
        });
    }

    #[test]
    fn launch_app_label_and_app_summary() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "launch_app",
                &json!({"goal": "打开微信", "app": "WeChat", "action": "启动微信"}),
            );
            assert_eq!(d.label, "启动应用");
            assert_eq!(d.summary, "WeChat");
        });
    }

    #[test]
    fn list_apps_label_and_goal_summary() {
        with_zh_cn_locale(|| {
            let d = default_display("list_apps", &json!({"goal": "查找微信"}));
            assert_eq!(d.label, "列出应用");
            assert_eq!(d.summary, "查找微信");
        });
    }

    #[test]
    fn cron_job_create_label_and_schedule_summary() {
        with_zh_cn_locale(|| {
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
        });
    }

    #[test]
    fn cron_job_delete_uses_job_id_summary() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "cron_job",
                &json!({"action": "delete", "job_id": "cron-abc123"}),
            );
            assert_eq!(d.label, "删除定时任务");
            assert_eq!(d.summary, "");
        });
    }

    #[test]
    fn cron_job_delete_with_label_summary() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "cron_job",
                &json!({"action": "delete", "job_id": "cron-abc123", "label": "每分钟提醒"}),
            );
            assert_eq!(d.label, "删除定时任务");
            assert_eq!(d.summary, "每分钟提醒");
        });
    }

    #[test]
    fn terminal_label_takes_priority_over_command() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "terminal",
                &json!({"command": "cargo test -p pointer-core task_board::", "label": "运行 task_board 单元测试"}),
            );
            assert_eq!(d.label, "终端命令");
            assert_eq!(d.summary, "运行 task_board 单元测试");
        });
    }

    #[test]
    fn terminal_falls_back_to_command_when_label_absent() {
        with_zh_cn_locale(|| {
            let d = default_display("terminal", &json!({"command": "cargo build --release"}));
            assert_eq!(d.label, "终端命令");
            assert_eq!(d.summary, "cargo build --release");
        });
    }

    #[test]
    fn ask_user_formats_question_and_options_summary() {
        with_zh_cn_locale(|| {
            let d = default_display(
                "ask_user",
                &json!({
                    "question": "是否允许桌面控制？",
                    "options": [{"label": "允许"}, {"label": "仅步骤"}]
                }),
            );
            assert_eq!(d.label, "询问用户");
            assert_eq!(d.summary, "是否允许桌面控制？\n1. 允许\n2. 仅步骤");
        });
    }

    #[test]
    fn job_await_uses_wait_label() {
        with_zh_cn_locale(|| {
            let d = default_display("job", &json!({"action": "await", "mode": "any"}));
            assert_eq!(d.label, "等待后台任务");
            assert!(d.summary.is_empty());
        });
    }

    #[test]
    fn default_display_renders_english_when_locale_is_en() {
        with_en_locale(|| {
            let d = default_display("file_read", &json!({"path": "src/App.vue"}));
            assert_eq!(d.label, "Read file");
        });
    }
}
