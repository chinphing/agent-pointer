use anyhow::{anyhow, Result};
use log::warn;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use super::terminal::TerminalStreamingResult;

pub fn terminal_requests_elevation(args: &serde_json::Value) -> bool {
    args.get("elevated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 用户会话不提供宿主提权。本地档位和沙箱档位都直接失败。
pub fn run_terminal_command_elevated(
    _args: serde_json::Value,
    _session_workspace: String,
    _on_output: impl Fn(&str) + Send,
    _cancel: Option<CancellationToken>,
    _run_abort: Option<Arc<AtomicBool>>,
    _session_user_id: Option<&str>,
) -> Result<TerminalStreamingResult> {
    warn!("terminal elevated: refused in a user session");
    Err(anyhow!("用户会话不提供宿主提权"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_requests_elevation_parses_bool() {
        assert!(!terminal_requests_elevation(
            &serde_json::json!({"command": "ls"})
        ));
        assert!(terminal_requests_elevation(
            &serde_json::json!({"command": "ls", "elevated": true})
        ));
        assert!(!terminal_requests_elevation(
            &serde_json::json!({"command": "ls", "elevated": false})
        ));
    }

    #[test]
    fn user_session_elevation_returns_error() {
        let result = run_terminal_command_elevated(
            serde_json::json!({"command": "id", "elevated": true}),
            String::new(),
            |_| {},
            None,
            None,
            None,
        );
        let Err(err) = result else {
            panic!("elevation must fail");
        };
        assert!(
            err.to_string().contains("用户会话不提供宿主提权"),
            "{err:#}"
        );
    }
}
