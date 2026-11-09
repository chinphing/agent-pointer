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
        // 编号来自 `launch_app` 的返回（`display 3 …`），这里不再重新找应用：
        // 重新解析一遍既慢，又可能因为窗口刚移动而和刚才那块屏不一致。
        //
        // 参数名统一是 `display`（模型可见层）；代码里的 `monitor` 是 xcap 的
        // 类型名，两者不混用。
        let wanted = args
            .get("display")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let monitors = screen::list_monitors()?;
        let target = match &wanted {
            Some(id) => monitors.iter().find(|m| m.id == id.trim()),
            None => None,
        };
        if let (Some(id), None) = (&wanted, target) {
            let known: Vec<&str> = monitors.iter().map(|m| m.id.as_str()).collect();
            return Ok(format!(
                "Goal: {goal}. No display with id `{id}`. Known ids: {}. \
                 Take the id from a `launch_app` reply.",
                known.join(", ")
            ));
        }
        let target = target.or_else(|| monitors.iter().find(|m| m.is_primary));

        let mut body = String::from("Displays:\n");
        for m in &monitors {
            // 只报编号与尺寸，**不报绝对坐标**：`bbox` 换算用图片像素，
            // 混进全局坐标会让模型拿错参考系。
            body.push_str(&format!(
                "- id={id} {w}×{h}{primary}{chosen}\n",
                id = m.id,
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

        let where_ = match target {
            Some(t) => format!("Capture will move to display {}.", t.id),
            None => "No display matched; capture stays where it is.".to_string(),
        };

        let next = if target.is_some() {
            "The next `[CUR_SCREEN]` frame captures that display — do not take your own \
             screenshots with `terminal`."
        } else {
            "Call `launch_app` for the target app and use the display id from its reply."
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

    /// 换屏直接用 `launch_app` 报回来的编号，**不重新找应用**：
    /// 解析结果就是传入的那个 id，与前台应用无关。
    #[test]
    fn monitor_id_is_taken_verbatim() {
        let args = json!({ "goal": "看 Chrome", "display": "3" });
        assert_eq!(
            super::super::tool_app_access::resolve_monitor_for_tool("use_screen", &args),
            Some("3".to_string())
        );
    }

    /// 两端空白要容忍：模型抄回来时可能带空格。
    #[test]
    fn monitor_id_is_trimmed() {
        let args = json!({ "goal": "看 Chrome", "display": " 3 " });
        assert_eq!(
            super::super::tool_app_access::resolve_monitor_for_tool("use_screen", &args),
            Some("3".to_string())
        );
    }

    /// 没给编号时不要瞎猜某块屏（交给调用方报错或保持现状）。
    #[test]
    fn missing_monitor_resolves_to_nothing() {
        let args = json!({ "goal": "看 Chrome" });
        assert_eq!(
            super::super::tool_app_access::resolve_monitor_for_tool("use_screen", &args),
            None
        );
    }
}
