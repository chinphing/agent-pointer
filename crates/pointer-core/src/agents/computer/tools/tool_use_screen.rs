//! `use_screen` —— 把采集切到目标窗口所在的那块显示器。
//!
//! 存在的理由：`[CUR_SCREEN]` 只截**一块**显示器。目标应用开在另一块屏时，
//! 模型看到的画面里根本没有它，于是会去用 `terminal` 跑 `screencapture` 自己找
//! ——那既慢又绕，还可能点到别的东西。这里给一个明确的动作：说清楚要看哪块屏。
//!
//! 真正的切换在宿主侧完成（`computer_monitor_follow`）：工具执行完，宿主把会话
//! 的采集显示器改成这里解析出来的那块，下一帧 `[CUR_SCREEN]` 就是它。

use super::args_util::require_non_empty_str;
use crate::agents::computer::vision::screen;
use crate::platform::app_access;
use anyhow::Result;
use serde_json::Value;

pub struct UseScreenTool;

impl UseScreenTool {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let goal = args["goal"].as_str().unwrap_or("").trim();
        let app = args
            .get("app")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let monitors = screen::list_monitors()?;
        let target_id = match app {
            Some(app) => app_access::monitor_id_for_launched_app(app),
            None => app_access::monitor_id_for_frontmost_app(),
        };
        let target = target_id
            .as_deref()
            .and_then(|id| monitors.iter().find(|m| m.id == id))
            .or_else(|| monitors.iter().find(|m| m.is_primary));

        let mut body = String::from("Displays:\n");
        for m in &monitors {
            body.push_str(&format!(
                "- id={id} bounds=({left},{top}) {w}x{h}{primary}{chosen}\n",
                id = m.id,
                left = m.left,
                top = m.top,
                w = m.width,
                h = m.height,
                primary = if m.is_primary { " primary" } else { "" },
                chosen = if Some(&m.id) == target.map(|t| &t.id) {
                    " ← capture switches here"
                } else {
                    ""
                },
            ));
        }

        let where_ = match (app, target) {
            (Some(app), Some(t)) => format!("`{app}` sits on display {}.", t.id),
            (None, Some(t)) => format!("The frontmost app sits on display {}.", t.id),
            (Some(app), None) => format!(
                "Could not locate a window for `{app}`; no display will be switched to."
            ),
            (None, None) => "Could not locate the frontmost app's display.".to_string(),
        };

        let next = if target.is_some() {
            "The next `[CUR_SCREEN]` frame captures that display — do not take your own \
             screenshots with `terminal`."
        } else {
            "Ask which display to use, or call `list_apps` first."
        };

        Ok(format!("Goal: {goal}. {where_}\n\n{body}\n{next}"))
    }
}

impl Default for UseScreenTool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `goal` 是必填：没有目标就没法判断该切哪块屏。
    #[test]
    fn requires_goal() {
        let err = UseScreenTool::new().execute(&json!({})).unwrap_err();
        assert!(err.to_string().contains("goal"), "旧字段名也要出现在报错里");
    }
}
