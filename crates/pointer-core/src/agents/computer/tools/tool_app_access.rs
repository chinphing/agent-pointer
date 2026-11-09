//! Application list/launch via OS accessibility (Codex Computer Use aligned).

use super::args_util::require_non_empty_str;
use crate::agents::computer::verify::VerifyHintGenerator;
use crate::platform::app_access::{
    self, render_list, AppOpenOptions, AppOpenResult, ListAppsOptions,
};
use anyhow::{anyhow, Result};
use log::info;
use serde_json::Value;

pub struct AppAccessTool {
    verify: VerifyHintGenerator,
}

impl AppAccessTool {
    pub fn new() -> Self {
        Self {
            verify: VerifyHintGenerator::new(),
        }
    }

    pub fn execute_list_apps(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;

        let include_all = args
            .get("include_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let options = ListAppsOptions { include_all };
        let apps = app_access::list_apps(options)?;
        info!("list_apps: count={} include_all={include_all}", apps.len());

        let goal = args["goal"].as_str().unwrap_or("").trim();
        let macos = cfg!(target_os = "macos");
        let body = render_list(&apps, macos);
        let body = if body.trim().is_empty() {
            if cfg!(windows) {
                "No running top-level apps are visible to this Windows runtime.".to_string()
            } else if cfg!(target_os = "linux") {
                "No running top-level apps are visible to this Linux runtime.".to_string()
            } else {
                "No apps matched the Codex app catalog.".to_string()
            }
        } else {
            body
        };

        Ok(format!(
            "Goal: {goal}. Listed {count} app(s). Pick the target app identifier from the lines below, then call launch_app with `app` set to the name or bundle/executable id:\n\n{body}\n\n{hint}",
            goal = goal,
            count = apps.len(),
            body = body,
            hint = self.verify.list_apps_hint(apps.len(), include_all),
        ))
    }

    pub fn execute_launch_app(&self, args: &Value) -> Result<String> {
        require_non_empty_str(args, "goal")?;
        let app = args
            .get("app")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Missing or empty 'app' for launch_app"))?;
        let activate_only = args
            .get("activate_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let new_instance = args
            .get("new_instance")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let options = AppOpenOptions {
            activate_only,
            new_instance,
        };
        options.validate().map_err(anyhow::Error::msg)?;

        let result = app_access::launch_app(app, options)?;
        info!(
            "launch_app: app={app} action={} success={}",
            result.action, result.success
        );
        Ok(format_launch_reply(args, &result, &self.verify))
    }
}

impl Default for AppAccessTool {
    fn default() -> Self {
        Self::new()
    }
}

fn format_launch_reply(
    args: &Value,
    result: &AppOpenResult,
    verify: &VerifyHintGenerator,
) -> String {
    let goal = args["goal"].as_str().unwrap_or("").trim();
    let app = args["app"].as_str().unwrap_or("").trim();
    let display_app = result.app_name.as_deref().unwrap_or(app);
    let status = if result.success { "OK" } else { "FAILED" };
    // 告诉模型它在哪块屏：双屏下「看不见刚打开的应用」多半就是采集在另一块屏。
    // 列不出显示器列表时也要报编号，只是少了尺寸。
    let screen = monitor_id_for_launched_app(args)
        .map(|id| match describe_monitor(&id) {
            Some(d) => format!(" It is on {d}."),
            None => format!(" It is on display {id}."),
        })
        .unwrap_or_default();
    format!(
        "Goal: {goal}. {status} — {} (action: {}).{screen} {}",
        result.message,
        result.action,
        verify.launch_app_hint(display_app, result.success, &result.action),
    )
}

/// Host-layer outcome parsed from `list_apps` / `launch_app` tool text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AppAccessHostOutcome {
    Pass,
    Fail,
}

/// Parse OS-level verification encoded in tool reply text.
/// `None` when the text is missing or does not match the expected shape.
#[allow(dead_code)]
pub fn parse_app_access_host_outcome(tool_name: &str, text: &str) -> Option<AppAccessHostOutcome> {
    let lower = text.to_lowercase();
    match tool_name.trim().to_ascii_lowercase().as_str() {
        "launch_app" => {
            if lower.contains(". failed —") || lower.contains("verification failed") {
                return Some(AppAccessHostOutcome::Fail);
            }
            if lower.contains(". ok —") && lower.contains("verified:") {
                return Some(AppAccessHostOutcome::Pass);
            }
            None
        }
        "list_apps" => {
            if lower.contains("listed") && lower.contains("app(s)") {
                return Some(AppAccessHostOutcome::Pass);
            }
            None
        }
        _ => None,
    }
}

#[allow(dead_code)]
pub fn app_access_host_pass_summary(tool_name: &str, text: &str) -> String {
    match tool_name.trim() {
        "launch_app" => {
            let first = text.lines().next().unwrap_or(text).trim();
            if first.is_empty() {
                "Host verified: app launch/activate succeeded.".to_string()
            } else {
                first.to_string()
            }
        }
        "list_apps" => "Host verified: app list returned.".to_string(),
        _ => "Host verified: app access step succeeded.".to_string(),
    }
}

/// 这次工具调用该把采集切到哪块屏 —— `use_screen` 与「自动跟随」共用一套解析，
/// 免得同一个判断在两个地方各写一遍、日后漂移。
pub fn resolve_monitor_for_tool(tool_id: &str, args: &Value) -> Option<String> {
    match tool_id {
        // 显式换屏：模型传回 `launch_app` / `[CUR_SCREEN]` 里看到的编号，
        // 可能是裸编号（`3`）也可能带前缀（`xcap:3`）——统一还原成存储用的规范 id。
        "use_screen" => args
            .get("monitor")
            .or_else(|| args.get("display"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(canonical_monitor_id),
        "launch_app" => monitor_id_for_launched_app(args),
        _ => crate::platform::app_access::monitor_id_for_frontmost_app(),
    }
}

/// 把模型写的编号还原成存储里的规范形式（`xcap:{id}`）。
///
/// 模型看到的是去掉前缀的编号，直接拿它去写会话状态会让 UI 选择器对不上号
/// （那边比的是完整字符串）。列不出显示器时原样返回，交给下游判失效。
fn canonical_monitor_id(raw: &str) -> String {
    let wanted = crate::agents::computer::vision::screen::normalize_monitor_id(raw);
    let Ok(monitors) = crate::agents::computer::vision::screen::list_monitors() else {
        return raw.to_string();
    };
    monitors
        .iter()
        .find(|m| crate::agents::computer::vision::screen::normalize_monitor_id(&m.id) == wanted)
        .map(|m| m.id.clone())
        .unwrap_or_else(|| raw.to_string())
}

/// 一句话说明某块屏的大小，用于工具返回（模型据此判断要不要换屏）。
///
/// **不带绝对坐标**：`bbox` 换算用的是图片像素，混进全局坐标只会让模型
/// 拿错参考系。也**不带 `xcap:` 前缀**：模型只需原样传回编号。
pub fn describe_monitor(monitor_id: &str) -> Option<String> {
    let monitors = crate::agents::computer::vision::screen::list_monitors().ok()?;
    let wanted = crate::agents::computer::vision::screen::normalize_monitor_id(monitor_id);
    let m = monitors
        .iter()
        .find(|m| crate::agents::computer::vision::screen::normalize_monitor_id(&m.id) == wanted)?;
    Some(format!(
        "display {} ({}×{})",
        crate::agents::computer::vision::screen::normalize_monitor_id(&m.id),
        m.width,
        m.height
    ))
}

pub fn monitor_id_for_launched_app(args: &Value) -> Option<String> {
    let app = args
        .get("app")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    crate::platform::app_access::monitor_id_for_launched_app(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_app_host_pass_wechat_activate() {
        let text = "Goal: 打开微信应用. OK — Activated \"WeChat\". Verified: target app is frontmost. (action: activate). launch_app activate for \"WeChat\" reported success with host verification.";
        assert_eq!(
            parse_app_access_host_outcome("launch_app", text),
            Some(AppAccessHostOutcome::Pass)
        );
    }

    #[test]
    fn launch_app_host_fail_not_frontmost() {
        let text = "Goal: open WeChat. FAILED — Activated \"WeChat\". Verification failed: app is running but not frontmost (tray-only or hidden). (action: activate). launch_app activate for \"WeChat\" failed host verification.";
        assert_eq!(
            parse_app_access_host_outcome("launch_app", text),
            Some(AppAccessHostOutcome::Fail)
        );
    }
}
