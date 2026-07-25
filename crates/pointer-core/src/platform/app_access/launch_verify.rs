//! Post-launch verification — confirm the target app is running / frontmost after launch_app.

use super::types::AppOpenResult;

pub const LAUNCH_VERIFY_POLL_MS: u64 = 250;
pub const LAUNCH_VERIFY_TIMEOUT_MS: u64 = 4000;
pub const ACTIVATE_VERIFY_TIMEOUT_MS: u64 = 2000;

/// Which launch_app code path is being verified (controls timeout only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchVerifyKind {
    Activate,
    Launch,
}

impl LaunchVerifyKind {
    pub fn timeout_ms(self) -> u64 {
        match self {
            Self::Activate => ACTIVATE_VERIFY_TIMEOUT_MS,
            Self::Launch => LAUNCH_VERIFY_TIMEOUT_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchVerifyOutcome {
    pub running: bool,
    pub frontmost: bool,
}

impl LaunchVerifyOutcome {
    pub fn not_found() -> Self {
        Self {
            running: false,
            frontmost: false,
        }
    }
}

/// Apply host verification to a launch/activate result and adjust success + message.
pub fn apply_launch_verification(result: &mut AppOpenResult, verification: LaunchVerifyOutcome) {
    if !result.success {
        return;
    }
    match result.action.as_str() {
        "activate" => {
            if verification.frontmost {
                result.message = format!(
                    "{} Verified: target app is frontmost.",
                    result.message.trim_end_matches('.')
                );
                log::info!(
                    "launch_app verify: activate confirmed frontmost app={:?}",
                    result.app_name
                );
            } else if verification.running {
                result.success = false;
                result.message = format!(
                    "{} Verification failed: app is running but not frontmost (tray-only or hidden).",
                    result.message.trim_end_matches('.')
                );
                log::warn!(
                    "launch_app verify: activate not frontmost app={:?}",
                    result.app_name
                );
            } else {
                result.success = false;
                result.message = format!(
                    "{} Verification failed: app not detected after activate.",
                    result.message.trim_end_matches('.')
                );
                log::warn!(
                    "launch_app verify: activate not running app={:?}",
                    result.app_name
                );
            }
        }
        "launch" => {
            if verification.running && verification.frontmost {
                result.message = format!(
                    "{} Verified: app is running and frontmost.",
                    result.message.trim_end_matches('.')
                );
                log::info!(
                    "launch_app verify: launch confirmed frontmost app={:?}",
                    result.app_name
                );
            } else if verification.running {
                result.success = false;
                result.message = format!(
                    "{} Verification failed: app is running but not frontmost (tray-only or hidden).",
                    result.message.trim_end_matches('.')
                );
                log::warn!(
                    "launch_app verify: launch not frontmost app={:?}",
                    result.app_name
                );
            } else {
                result.success = false;
                result.message = format!(
                    "{} Verification failed: app window not detected within timeout.",
                    result.message.trim_end_matches('.')
                );
                log::warn!(
                    "launch_app verify: launch not detected app={:?}",
                    result.app_name
                );
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_fails_when_not_frontmost() {
        let mut result = AppOpenResult {
            success: true,
            action: "activate".into(),
            message: "Activated \"WeChat\".".into(),
            app_name: Some("WeChat".into()),
        };
        apply_launch_verification(
            &mut result,
            LaunchVerifyOutcome {
                running: true,
                frontmost: false,
            },
        );
        assert!(!result.success);
        assert!(result.message.contains("not frontmost"));
    }

    #[test]
    fn launch_fails_when_running_not_frontmost() {
        let mut result = AppOpenResult {
            success: true,
            action: "launch".into(),
            message: "Launched \"WeChat\".".into(),
            app_name: Some("WeChat".into()),
        };
        apply_launch_verification(
            &mut result,
            LaunchVerifyOutcome {
                running: true,
                frontmost: false,
            },
        );
        assert!(!result.success);
        assert!(result.message.contains("not frontmost"));
    }

    #[test]
    fn launch_passes_when_running_and_frontmost() {
        let mut result = AppOpenResult {
            success: true,
            action: "launch".into(),
            message: "Launched \"WeChat\".".into(),
            app_name: Some("WeChat".into()),
        };
        apply_launch_verification(
            &mut result,
            LaunchVerifyOutcome {
                running: true,
                frontmost: true,
            },
        );
        assert!(result.success);
        assert!(result.message.contains("Verified"));
    }
}
