//! `use_screen` —— 把采集切到目标窗口所在的那块显示器。
//!
//! 存在的理由：`[CUR_SCREEN]` 只截**一块**显示器。目标应用开在另一块屏时，
//! 模型看到的画面里根本没有它，于是会去用 `terminal` 跑 `screencapture` 自己找
//! ——那既慢又绕，还可能点到别的东西。这里给一个明确的动作：说清楚要看哪块屏。
//!
//! 真正的切换在宿主侧完成（`computer_monitor_follow`）：工具执行完，宿主把会话
//! 的采集显示器改成这里解析出来的那块，下一帧 `[CUR_SCREEN]` 就是它。

use super::args_util::{display_id_from_args, require_non_empty_str};
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
        // 类型名，两者不混用。值可能是字符串 `"3"` 或数字 `3`，两种都收。
        let monitors = screen::list_monitors()?;
        // 任何一条失败路径都带上真实编号：模型据此自我纠正，不必再问一轮。
        let known = monitors
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let wanted = match args.get("display") {
            None => None,
            Some(v) => match display_id_from_args(Some(v)) {
                Some(id) => Some(id),
                // 传了但读不出来：明确报错，不能当成「没传」静默换到主屏。
                None => return Ok(invalid_display_reply(goal, v, &known)),
            },
        };

        let target = match &wanted {
            Some(id) => monitors.iter().find(|m| m.id == id.trim()),
            None => None,
        };
        if let (Some(id), None) = (&wanted, target) {
            return Ok(format!(
                "No display with id `{id}`. Known ids: {known}. \
                 Take the id from a `launch_app` reply. No capture change was made."
            ));
        }
        // 没传 `display`：只列出显示器，**不改采集**。
        // （原来这里会 or_else 到主屏，等于"省略即切主屏"，与文档说的「保持当前」相反。）
        let target = match &wanted {
            Some(_) => target,
            None => None,
        };

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

        let where_ = match (target, &wanted) {
            (Some(t), _) => format!("Capture will move to display {}.", t.id),
            (None, None) => "No `display` was given — capture stays where it is.".to_string(),
            (None, Some(_)) => "No display matched; capture stays where it is.".to_string(),
        };

        let next = if target.is_some() {
            "The next `[CUR_SCREEN]` frame captures that display — do not take your own \
             screenshots with `terminal`."
        } else {
            "Call `launch_app` for the target app and use the display id from its reply."
        };

        Ok(format!("{where_}\n\n{body}\n{next}"))
    }
}

impl Default for UseScreenTool {
    fn default() -> Self {
        Self::new()
    }
}

/// `display` 读不出来时的回复：带上真实编号，模型可直接改对再调一次。
fn invalid_display_reply(goal: &str, got: &Value, known: &str) -> String {
    format!(
        "Goal: {goal}. `display` must be a display id such as 3, got `{got}`. \
         Known ids: {known}. No capture change was made."
    )
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

    /// 显示器 id 在 JSON 里天然是数字：模型写 `{"display": 3}` 也要认。
    /// 只认字符串会让它被当成「没传」，静默换到主屏。
    #[test]
    fn numeric_display_id_is_accepted() {
        let args = json!({ "goal": "看 Chrome", "display": 3 });
        assert_eq!(
            super::super::tool_app_access::resolve_monitor_for_tool("use_screen", &args),
            Some("3".to_string())
        );
    }

    /// 报错时必须把真实编号一并给出：模型能直接改对重调，不用再问一轮。
    #[test]
    fn invalid_display_reply_lists_known_ids() {
        let text = super::invalid_display_reply("看 Chrome", &json!({}), "1, 3");
        assert!(text.contains("Known ids: 1, 3"), "got: {text}");
        assert!(text.contains("No capture change was made"), "got: {text}");
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
