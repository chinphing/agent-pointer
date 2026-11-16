//! Shared subprocess runner for lint engines (wall timeout, output caps).
//!
//! 子进程一律通过 [`crate::env::Environment::exec`] 启动：`tools/` 与 `agents/` 层不再
//! 直接 `Command::new`，换后端（Cube / 远端 envd）时只改环境实现。
//! 超时与输出上限由 `ExecOptions` 表达，`ExecOutput` 回传 `timed_out` / `*_truncated`。

use anyhow::{anyhow, Result};
use crate::env::{effective_env, ExecOptions};
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

/// Captured process output (stdout and stderr may be truncated separately).
#[derive(Debug, Clone)]
pub struct CapturedOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

fn to_captured(out: crate::env::ExecOutput) -> CapturedOutput {
    CapturedOutput {
        stdout: out.stdout,
        stderr: out.stderr,
        stdout_truncated: out.stdout_truncated,
        stderr_truncated: out.stderr_truncated,
        // 被信号杀死 / 超时终止时没有退出码，与 `ExitStatus::code()` 语义一致。
        exit_code: (out.status >= 0).then_some(out.status),
        timed_out: out.timed_out,
    }
}

/// Run `program` + `args` in `cwd` with wall-clock timeout. Merged stdout cap for line consumers.
pub fn run_argv_capture_lines(
    program: impl AsRef<OsStr>,
    args: &[String],
    cwd: &Path,
    wall_ms: u64,
    max_stdout_bytes: usize,
) -> Result<(Vec<String>, CapturedOutput)> {
    let mut argv: Vec<String> = Vec::with_capacity(args.len() + 1);
    argv.push(program.as_ref().to_string_lossy().into_owned());
    argv.extend(args.iter().cloned());

    let out = effective_env().exec(
        &argv,
        ExecOptions::new()
            .with_cwd(cwd)
            .with_timeout(Duration::from_millis(wall_ms))
            .with_max_output_bytes(max_stdout_bytes),
    )?;
    let captured = to_captured(out);
    let stdout_str = String::from_utf8_lossy(&captured.stdout);
    let lines: Vec<String> = stdout_str.lines().map(|s| s.to_string()).collect();
    Ok((lines, captured))
}

/// Run a shell one-liner (same spirit as `terminal` tool: `cmd /C` on Windows, `sh -lc` elsewhere).
pub fn run_shell_capture(
    cwd: &Path,
    shell_cmd: &str,
    wall_ms: u64,
    max_stream_bytes: usize,
) -> Result<CapturedOutput> {
    let argv: Vec<String> = if cfg!(windows) {
        vec![
            "cmd.exe".to_string(),
            "/C".to_string(),
            crate::windows_shell_encoding::prefix_cmd_utf8_codepage(shell_cmd),
        ]
    } else {
        vec!["sh".to_string(), "-lc".to_string(), shell_cmd.to_string()]
    };

    let out = effective_env()
        .exec(
            &argv,
            ExecOptions::new()
                .with_cwd(cwd)
                .with_timeout(Duration::from_millis(wall_ms))
                .with_max_output_bytes(max_stream_bytes),
        )
        .map_err(|e| anyhow!("无法启动 shell 执行 lint 命令: {e}"))?;
    Ok(to_captured(out))
}
