//! Shared types for cross-platform application launch.

use serde::{Deserialize, Serialize};

/// Options for [`super::list_apps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListAppsOptions {
    /// When true, include the full installed application catalog.
    /// Default (false): running apps plus entries used in the last 14 days only.
    pub include_all: bool,
}

/// Options for [`super::launch_app`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppOpenOptions {
    /// Only bring a running instance to the foreground; never launch if none is running.
    pub activate_only: bool,
    /// Skip activation and start a new instance (user explicitly asked for another window/process).
    pub new_instance: bool,
}

impl AppOpenOptions {
    pub fn validate(self) -> Result<(), String> {
        if self.activate_only && self.new_instance {
            return Err("activate_only and new_instance cannot both be true".into());
        }
        Ok(())
    }
}

/// Result of launching or focusing an application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppOpenResult {
    pub success: bool,
    pub action: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_options_rejects_conflicting_flags() {
        assert!(AppOpenOptions {
            activate_only: true,
            new_instance: true,
        }
        .validate()
        .is_err());
    }
}
