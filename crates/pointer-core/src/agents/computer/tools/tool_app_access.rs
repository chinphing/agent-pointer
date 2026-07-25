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
    format!(
        "Goal: {goal}. {status} — {} (action: {}). {}",
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

/// Map a launched app's main window to a capture monitor id, when detectable.
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
