use super::{ToolEntry, ToolHandler, ToolRegistry};
use anyhow::{anyhow, Result};
use log::warn;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 子进程不创建控制台窗口（避免 Windows 上执行 terminal 工具时闪出黑框 / PowerShell 控制台）。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const TERMINAL_DEFAULT_TIMEOUT_MS: u64 = 30_000;
const TERMINAL_MAX_TIMEOUT_MS: u64 = 120_000;
/// 自进程启动起的墙钟上限（与是否有输出无关）。
const TERMINAL_ABS_MAX_WALL_MS: u64 = 3_600_000;
const TERMINAL_DEFAULT_MAX_OUTPUT_BYTES: usize = 20_000;
const TERMINAL_MAX_OUTPUT_BYTES: usize = 200_000;

pub fn register_all(reg: &ToolRegistry) {
    register_terminal(reg);
}

fn register_terminal(reg: &ToolRegistry) {
    const DOC_SOURCE: &str = "tools/prompts/terminal.md";
    let doc = include_str!("prompts/terminal.md").trim();
    let h: ToolHandler = Arc::new(run_terminal_command);
    reg.register(ToolEntry::new(
        "terminal",
        DOC_SOURCE,
        "high",
        true,
        doc,
        h,
    ));
}

fn effective_terminal_cwd(explicit: Option<PathBuf>) -> Result<Option<PathBuf>> {
    if explicit.is_some() {
        return Ok(explicit);
    }
    let w = crate::tools::file::workspace_root_from_override_or_settings();
    let w = w.trim();
    if !w.is_empty() {
        let p = PathBuf::from(w);
        if p.is_dir() {
            return Ok(Some(p.canonicalize().unwrap_or(p)));
        }
    }
    Ok(None)
}

/// 超时或需要强制结束时：在 Windows 上仅 `Child::kill` 往往只杀掉 shell（如 PowerShell），
/// 由其拉起的子进程会继续跑；用 `taskkill /T` 结束整棵进程树。其他平台仍用 `kill`。
fn kill_terminal_child_tree_best_effort(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        if pid > 0 {
            let _ = Command::new("taskkill.exe")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .status();
        }
    }
    let _ = child.kill();
}

fn run_terminal_command(args: serde_json::Value) -> Result<String> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("缺少 command"))?;
    let cwd = effective_terminal_cwd(parse_terminal_cwd(args.get("cwd"))?)?;
    let max_output_bytes = args
        .get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_MAX_OUTPUT_BYTES as u64)
        .min(TERMINAL_MAX_OUTPUT_BYTES as u64) as usize;
    let (shell, _) = terminal_shell_command(&command);

    let r = run_terminal_command_streaming(args, |_| {}, None, None)?;

    Ok(serde_json::json!({
        "command": command.as_str(),
        "cwd": cwd.map(|p| p.display().to_string()).unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()),
        "shell": shell,
        "exitCode": r.exit_code,
        "success": r.success,
        "timedOut": r.timed_out,
        "cancelled": r.cancelled,
        "runAborted": r.run_aborted,
        "durationMs": r.duration_ms,
        "stdout": r.stdout,
        "stderr": r.stderr,
        "stdoutTruncated": r.stdout_truncated,
        "stderrTruncated": r.stderr_truncated,
        "maxOutputBytes": max_output_bytes
    })
    .to_string())
}

pub struct TerminalStreamingResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    /// User stopped the assistant turn (or equivalent cancel token fired).
    pub cancelled: bool,
    /// Host requested abort of this terminal run only (conversation still active).
    pub run_aborted: bool,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Run a terminal command with optional cooperative cancel.
/// - `cancel`: whole turn stopped (e.g. user "stop generation").
/// - `run_abort`: only this subprocess should stop (`Arc<AtomicBool>` set by host).
pub fn run_terminal_command_streaming(
    args: serde_json::Value,
    on_output: impl Fn(&str) + Send,
    cancel: Option<CancellationToken>,
    run_abort: Option<Arc<AtomicBool>>,
) -> Result<TerminalStreamingResult> {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("缺少 command"))?;
    let cwd = effective_terminal_cwd(parse_terminal_cwd(args.get("cwd"))?)?;
    let timeout_ms = args
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_TIMEOUT_MS)
        .clamp(1_000, TERMINAL_MAX_TIMEOUT_MS);
    let wall_cap_ms = args
        .get("maxWallMs")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(1_000, TERMINAL_ABS_MAX_WALL_MS))
        .unwrap_or(TERMINAL_ABS_MAX_WALL_MS);
    let max_output_bytes = args
        .get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_MAX_OUTPUT_BYTES as u64)
        .min(TERMINAL_MAX_OUTPUT_BYTES as u64) as usize;

    let (_shell, mut cmd) = terminal_shell_command(command);
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let started = Instant::now();
    let mut last_output_at = started;
    let mut child = cmd.spawn().map_err(|e| anyhow!("启动终端命令失败: {e}"))?;

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("无法获取 stdout"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("无法获取 stderr"))?;

    let (tx, rx) = mpsc::channel::<TerminalPipeChunk>();
    spawn_pipe_reader(stdout_pipe, TerminalPipe::Stdout, tx.clone());
    spawn_pipe_reader(stderr_pipe, TerminalPipe::Stderr, tx);

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut timed_out = false;
    let mut cancelled = false;
    let mut run_aborted = false;
    let status = loop {
        while let Ok(chunk) = rx.try_recv() {
            last_output_at = Instant::now();
            on_output(&chunk.text);
            match chunk.pipe {
                TerminalPipe::Stdout => stdout_buf.push_str(&chunk.text),
                TerminalPipe::Stderr => stderr_buf.push_str(&chunk.text),
            }
        }

        if let Some(status) = child.try_wait()? {
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            if !status.success() {
                let code = status.code().unwrap_or(-1);
                on_output(&format!("\n进程退出，退出码: {code}\n"));
            }
            break status;
        }

        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            cancelled = true;
            warn!("terminal: session cancelled; killing child process tree");
            kill_terminal_child_tree_best_effort(&mut child);
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已因会话取消被终止\n");
            break status;
        }

        if run_abort
            .as_ref()
            .is_some_and(|a| a.load(Ordering::SeqCst))
        {
            run_aborted = true;
            warn!("terminal: run-only abort; killing child process tree");
            kill_terminal_child_tree_best_effort(&mut child);
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已由宿主仅终止当前终端命令\n");
            break status;
        }

        if started.elapsed() >= Duration::from_millis(wall_cap_ms) {
            timed_out = true;
            kill_terminal_child_tree_best_effort(&mut child);
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已超时（达到墙钟上限），已终止执行\n");
            break status;
        }

        if last_output_at.elapsed() >= Duration::from_millis(timeout_ms) {
            timed_out = true;
            kill_terminal_child_tree_best_effort(&mut child);
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已超时（长时间无输出），已终止执行\n");
            break status;
        }

        thread::sleep(Duration::from_millis(50));
    };

    drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);

    let duration_ms = started.elapsed().as_millis() as u64;
    let (stdout, stdout_truncated) = truncate_output(stdout_buf.as_bytes(), max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(stderr_buf.as_bytes(), max_output_bytes);

    Ok(TerminalStreamingResult {
        exit_code: status.code(),
        success: status.success() && !timed_out && !cancelled && !run_aborted,
        timed_out,
        cancelled,
        run_aborted,
        duration_ms,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

#[derive(Clone, Copy)]
enum TerminalPipe {
    Stdout,
    Stderr,
}

struct TerminalPipeChunk {
    pipe: TerminalPipe,
    text: String,
}

fn spawn_pipe_reader<R>(reader: R, pipe: TerminalPipe, tx: mpsc::Sender<TerminalPipeChunk>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = tx.send(TerminalPipeChunk {
                        pipe,
                        text: line.clone(),
                    });
                }
                Err(_) => break,
            }
        }
    });
}

fn drain_pipe_chunks(
    rx: &mpsc::Receiver<TerminalPipeChunk>,
    stdout_buf: &mut String,
    stderr_buf: &mut String,
    on_output: &impl Fn(&str),
) {
    while let Ok(chunk) = rx.try_recv() {
        on_output(&chunk.text);
        match chunk.pipe {
            TerminalPipe::Stdout => stdout_buf.push_str(&chunk.text),
            TerminalPipe::Stderr => stderr_buf.push_str(&chunk.text),
        }
    }
}

fn parse_terminal_cwd(value: Option<&serde_json::Value>) -> Result<Option<PathBuf>> {
    let Some(raw) = value.and_then(|v| v.as_str()).map(str::trim) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(raw);
    if !path.exists() {
        return Err(anyhow!("cwd 不存在: {raw}"));
    }
    if !path.is_dir() {
        return Err(anyhow!("cwd 不是目录: {raw}"));
    }
    Ok(Some(path))
}

#[cfg(windows)]
fn terminal_shell_command(command: &str) -> (&'static str, Command) {
    let mut cmd = Command::new("powershell");
    cmd.arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(command);
    (
        "powershell -ExecutionPolicy Bypass -Command",
        cmd,
    )
}

#[cfg(not(windows))]
fn terminal_shell_command(command: &str) -> (&'static str, Command) {
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(command);
    ("sh -lc", cmd)
}

/// Tool-call UI: timeout always fails; otherwise `Some(0)` ⇒ success.
pub fn terminal_stream_tool_status(r: &TerminalStreamingResult) -> (bool, Option<String>) {
    if r.cancelled {
        return (false, Some("命令已因会话取消被终止".to_string()));
    }
    if r.run_aborted {
        return (false, Some("命令已由宿主终止（仅结束当前终端）".to_string()));
    }
    if r.timed_out {
        return (false, Some("命令执行超时".to_string()));
    }
    if matches!(r.exit_code, Some(0)) {
        return (true, None);
    }
    let msg = match r.exit_code {
        Some(n) => format!("命令失败（退出码 {n}）"),
        None => "命令失败（无退出码）".to_string(),
    };
    (false, Some(msg))
}

fn truncate_output(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }
    let mut end = max_bytes.min(bytes.len());
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    let mut text = String::from_utf8_lossy(&bytes[..end]).to_string();
    text.push_str("\n...[output truncated]");
    (text, true)
}
