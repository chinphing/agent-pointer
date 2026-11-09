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
        // 给了编号且这块屏存在 → 一句话确认。显示器清单、编号、下一帧会截哪块屏，
        // 模型都已经知道（编号就是它从 `launch_app` 抄来的），再说一遍只是噪声。
        if let Some(t) = target {
            return Ok(switched_reply(&t.id));
        }

        // 没传 `display`：只列出编号，**不改采集**。
        // （原来这里会 or_else 到主屏，等于"省略即切主屏"，与文档说的「保持当前」相反。）
        //
        // 不报尺寸也不报绝对坐标：换屏收的是编号；`bbox` 换算用当前截图的像素，
        // 混进别的数字只会让模型拿错参考系。
        Ok(format!(
            "No `display` was given — capture stays where it is.\n\n{}\
             Call `launch_app` for the target app and use the display id from its reply.",
            display_list_reply(&monitors)
        ))
    }
}

impl Default for UseScreenTool {
    fn default() -> Self {
        Self::new()
    }
}

/// 显示器清单：**只给编号**（外加 `primary` 标记）。
///
/// 不报尺寸、不报绝对坐标 —— 换屏收的是编号，`bbox` 换算用当前截图的像素，
/// 多给数字只会让模型拿错参考系。
fn display_list_reply(monitors: &[crate::models::ComputerMonitor]) -> String {
    let mut body = String::from("Displays:\n");
    for m in monitors {
        body.push_str(&format!(
            "- {id}{primary}\n",
            id = m.id,
            primary = if m.is_primary { " primary" } else { "" },
        ));
    }
    body
}

/// 换屏成功的回复：**只有这一句**。
///
/// 显示器清单、编号、下一帧会截哪块屏 —— 模型都已经知道（编号就是它从
/// `launch_app` 抄来的），再说一遍是噪声。
fn switched_reply(id: &str) -> String {
    format!("Switched to display {id}.")
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

    /// 列显示器时只给编号：尺寸对换屏没用，模型拿到编号就够了。
    #[test]
    fn display_list_reports_ids_only() {
        let text = super::display_list_reply(&[
            crate::models::ComputerMonitor {
                id: "1".into(),
                left: 0,
                top: 0,
                width: 1512,
                height: 982,
                is_primary: true,
                work_area: None,
            },
            crate::models::ComputerMonitor {
                id: "3".into(),
                left: 1512,
                top: 0,
                width: 3440,
                height: 1440,
                is_primary: false,
                work_area: None,
            },
        ]);
        assert_eq!(text, "Displays:\n- 1 primary\n- 3\n");
        assert!(!text.contains('×'), "不要报尺寸：{text}");
    }

    /// 成功回复只有一句，不能带清单或解释。
    #[test]
    fn success_reply_is_one_line() {
        let text = super::switched_reply("3");
        assert_eq!(text, "Switched to display 3.");
        assert_eq!(text.lines().count(), 1, "多行就是在重复模型已知的信息");
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
