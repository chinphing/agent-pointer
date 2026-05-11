use super::{ToolEntry, ToolHandler, ToolRegistry};
use crate::storage;
use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const TERMINAL_DEFAULT_TIMEOUT_MS: u64 = 30_000;
const TERMINAL_MAX_TIMEOUT_MS: u64 = 120_000;
const TERMINAL_DEFAULT_MAX_OUTPUT_BYTES: usize = 20_000;
const TERMINAL_MAX_OUTPUT_BYTES: usize = 200_000;

pub fn register_all(reg: &ToolRegistry) {
    register_terminal(reg);
}

fn register_terminal(reg: &ToolRegistry) {
    let doc = include_str!("prompts/terminal.md").trim();
    let h: ToolHandler = Arc::new(run_terminal_command);
    reg.register(ToolEntry::new(
        "terminal",
        "high",
        true,
        doc,
        None,
        h,
    ));
}

fn effective_terminal_cwd(explicit: Option<PathBuf>) -> Result<Option<PathBuf>> {
    if explicit.is_some() {
        return Ok(explicit);
    }
    if let Ok(s) = storage::load_settings() {
        let w = s.workspace_root.trim();
        if !w.is_empty() {
            let p = PathBuf::from(w);
            if p.is_dir() {
                return Ok(Some(p.canonicalize().unwrap_or(p)));
            }
        }
    }
    Ok(None)
}

fn run_terminal_command(args: serde_json::Value) -> Result<String> {
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
    let max_output_bytes = args
        .get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_MAX_OUTPUT_BYTES as u64)
        .min(TERMINAL_MAX_OUTPUT_BYTES as u64) as usize;

    let (shell, mut cmd) = terminal_shell_command(command);
    if let Some(dir) = &cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| anyhow!("启动终端命令失败: {e}"))?;
    let mut timed_out = false;

    loop {
        if let Some(_status) = child.try_wait()? {
            break;
        }
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let output = child
        .wait_with_output()
        .map_err(|e| anyhow!("读取终端命令输出失败: {e}"))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let (stdout, stdout_truncated) = truncate_output(&output.stdout, max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(&output.stderr, max_output_bytes);

    Ok(serde_json::json!({
        "command": command,
        "cwd": cwd.map(|p| p.display().to_string()).unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()),
        "shell": shell,
        "exitCode": output.status.code(),
        "success": output.status.success() && !timed_out,
        "timedOut": timed_out,
        "durationMs": duration_ms,
        "stdout": stdout,
        "stderr": stderr,
        "stdoutTruncated": stdout_truncated,
        "stderrTruncated": stderr_truncated,
        "maxOutputBytes": max_output_bytes
    })
    .to_string())
}

pub struct TerminalStreamingResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub fn run_terminal_command_streaming(
    args: serde_json::Value,
    on_output: impl Fn(&str) + Send,
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

    let started = Instant::now();
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
    let status = loop {
        while let Ok(chunk) = rx.try_recv() {
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

        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            timed_out = true;
            let _ = child.kill();
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已超时，已终止执行\n");
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
        success: status.success() && !timed_out,
        timed_out,
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
    cmd.arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(command);
    (
        "powershell -NoProfile -ExecutionPolicy Bypass -Command",
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
