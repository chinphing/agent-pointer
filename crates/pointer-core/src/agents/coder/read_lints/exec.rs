//! Shared subprocess runner for lint engines (wall timeout, output caps, Windows no-console).

use anyhow::{anyhow, Result};
use log::warn;
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
fn kill_child_process_tree(child: &mut std::process::Child) {
    let pid = child.id();
    if pid > 0 {
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
}

#[cfg(not(windows))]
fn kill_child_process_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}

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

fn copy_limited<R: Read>(mut r: R, cap: usize) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut truncated = false;
    loop {
        if buf.len() >= cap {
            truncated = true;
            break;
        }
        let n = match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let take = n.min(cap.saturating_sub(buf.len()));
        buf.extend_from_slice(&chunk[..take]);
        if take < n {
            truncated = true;
            break;
        }
    }
    (buf, truncated)
}

/// Run `program` + `args` in `cwd` with wall-clock timeout. Merged stdout cap for line consumers.
pub fn run_argv_capture_lines(
    program: impl AsRef<OsStr>,
    args: &[String],
    cwd: &Path,
    wall_ms: u64,
    max_stdout_bytes: usize,
) -> Result<(Vec<String>, CapturedOutput)> {
    let mut cmd = Command::new(program.as_ref());
    cmd.args(args).current_dir(cwd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("无法启动进程 {:?}: {e}", program.as_ref().to_string_lossy()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("无法读取 stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("无法读取 stderr"))?;

    let stdout_handle = thread::spawn(move || copy_limited(stdout, max_stdout_bytes));

    let stderr_handle = thread::spawn(move || {
        let stderr = stderr;
        copy_limited(stderr, max_stdout_bytes)
    });

    let start = Instant::now();
    let mut timed_out = false;
    let status = 'wait: loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() >= Duration::from_millis(wall_ms) {
                    timed_out = true;
                    warn!(
                        "read_lints exec: {} exceeded wall {} ms",
                        program.as_ref().to_string_lossy(),
                        wall_ms
                    );
                    kill_child_process_tree(&mut child);
                    break 'wait child
                        .wait()
                        .map_err(|e| anyhow!("等待子进程结束失败: {e}"))?;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(anyhow!("等待子进程失败: {e}")),
        }
    };

    let (stdout_bytes, stdout_truncated) = stdout_handle.join().unwrap_or((Vec::new(), false));
    let (stderr_bytes, stderr_truncated) = stderr_handle.join().unwrap_or((Vec::new(), false));

    let stdout_str = String::from_utf8_lossy(&stdout_bytes);
    let lines: Vec<String> = stdout_str.lines().map(|s| s.to_string()).collect();

    Ok((
        lines,
        CapturedOutput {
            stdout: stdout_bytes,
            stderr: stderr_bytes,
            stdout_truncated,
            stderr_truncated,
            exit_code: status.code(),
            timed_out,
        },
    ))
}

/// Run a shell one-liner (same spirit as `terminal` tool: `cmd /C` on Windows, `sh -lc` elsewhere).
pub fn run_shell_capture(
    cwd: &Path,
    shell_cmd: &str,
    wall_ms: u64,
    max_stream_bytes: usize,
) -> Result<CapturedOutput> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        c.args([
            "/C",
            &crate::windows_shell_encoding::prefix_cmd_utf8_codepage(shell_cmd),
        ])
        .current_dir(cwd);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-lc", shell_cmd]).current_dir(cwd);
        c
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("无法启动 shell 执行 lint 命令: {e}"))?;

    let stdout = child.stdout.take().ok_or_else(|| anyhow!("stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("stderr"))?;

    let stdout_handle = thread::spawn(move || copy_limited(stdout, max_stream_bytes));
    let stderr_handle = thread::spawn(move || copy_limited(stderr, max_stream_bytes));

    let start = Instant::now();
    let mut timed_out = false;
    let status = 'wait: loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() >= Duration::from_millis(wall_ms) {
                    timed_out = true;
                    kill_child_process_tree(&mut child);
                    break 'wait child
                        .wait()
                        .map_err(|e| anyhow!("等待 shell 结束失败: {e}"))?;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(anyhow!("等待 shell 失败: {e}")),
        }
    };

    let (stdout, stdout_truncated) = stdout_handle.join().unwrap_or((Vec::new(), false));
    let (stderr, stderr_truncated) = stderr_handle.join().unwrap_or((Vec::new(), false));

    Ok(CapturedOutput {
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        exit_code: status.code(),
        timed_out,
    })
}
