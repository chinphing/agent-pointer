#[cfg(unix)]
use super::terminal_askpass::{
    deliver_askpass_password, try_create_ssh_askpass, try_setup_ssh_askpass,
};
use super::terminal_elevated::run_terminal_command_elevated;
use super::terminal_prompt::{
    agent_retry_forbidden, detect_prompt_state, input_context_snippet,
    post_interactive_ssh_secret_prompt, scrub_secret_echo, PROMPT_DETECT_IDLE_MS,
};
use super::terminal_pty::{
    command_wants_pty, try_spawn_terminal_pty, ActiveChild, TerminalInputSink,
};
use super::{ToolEntry, ToolHandler, ToolRegistry};

pub(crate) use super::terminal_elevated::terminal_requests_elevation;
pub use super::terminal_prompt::InputClass;
use crate::dotenv::{
    apply_supplemental_env_files, default_user_env_file, parse_env_file_args, resolve_env_file_path,
};
use anyhow::{anyhow, bail, Result};
use log::{info, warn};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
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

const TERMINAL_DEFAULT_WAIT_FOR_INPUT_MS: u64 = 120_000;
const TERMINAL_MAX_WAIT_FOR_INPUT_MS: u64 = 600_000;

/// Ceiling for stdout/stderr returned to the model. Tool `maxOutputBytes` may only lower it.
pub(crate) fn resolve_max_output_bytes(args: &serde_json::Value) -> usize {
    let ceiling = crate::storage::load_user_settings()
        .ok()
        .map(|u| crate::models::clamp_terminal_output_max_bytes(u.terminal_output_max_bytes))
        .unwrap_or(crate::models::DEFAULT_TERMINAL_OUTPUT_MAX_BYTES) as u64;
    args.get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(ceiling)
        .clamp(1, ceiling) as usize
}

/// Idle timeout. Settings value is the default when `timeoutMs` is omitted.
/// Tool `timeoutMs` is clamped to 1s–86400s and is not capped by that default.
pub(crate) fn resolve_timeout_ms(args: &serde_json::Value) -> u64 {
    let default_ms = crate::storage::load_user_settings()
        .ok()
        .map(|u| crate::models::terminal_timeout_ms(u.terminal_timeout_seconds))
        .unwrap_or_else(|| {
            crate::models::terminal_timeout_ms(crate::models::DEFAULT_TERMINAL_TIMEOUT_SECONDS)
        });
    let hard_cap =
        crate::models::terminal_timeout_ms(crate::models::CEILING_TERMINAL_TIMEOUT_SECONDS);
    args.get("timeoutMs")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(1_000, hard_cap.max(1_000)))
        .unwrap_or(default_ms)
}

/// Wall-clock cap from settings (hours). Tool `maxWallMs` may only lower it.
pub(crate) fn resolve_max_wall_ms(args: &serde_json::Value) -> u64 {
    let ceiling = crate::storage::load_user_settings()
        .ok()
        .map(|u| crate::models::terminal_max_wall_ms(u.terminal_max_wall_hours))
        .unwrap_or_else(|| {
            crate::models::terminal_max_wall_ms(crate::models::DEFAULT_TERMINAL_MAX_WALL_HOURS)
        });
    args.get("maxWallMs")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(1_000, ceiling))
        .unwrap_or(ceiling)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalNeedsInputPrompt {
    pub request_id: String,
    pub command: String,
    pub output_context: Option<String>,
    pub input_hint: Option<String>,
    pub input_class: InputClass,
    pub wait_for_input_ms: u64,
}

#[derive(Debug, Clone)]
pub enum TerminalInputResolution {
    Submit(String),
    Dismiss,
    Cancelled,
}

pub struct TerminalInputHooks {
    pub prompt_tx: std::sync::mpsc::Sender<TerminalNeedsInputPrompt>,
    pub resolution_rx: std::sync::mpsc::Receiver<TerminalInputResolution>,
}

pub fn register_all(reg: &ToolRegistry) {
    register_terminal(reg);
}

fn register_terminal(reg: &ToolRegistry) {
    const DOC_SOURCE: &str = "tools/prompts/terminal.md";
    let doc = include_str!("prompts/terminal.md").trim();
    let h: ToolHandler = Arc::new(run_terminal_command);
    reg.register(ToolEntry::new("terminal", DOC_SOURCE, "high", true, doc, h));
}

pub(crate) fn effective_terminal_cwd(
    explicit: Option<PathBuf>,
    session_workspace: &str,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(normalize_terminal_cwd(path.canonicalize().unwrap_or(path)));
    }
    let trimmed = session_workspace.trim();
    if !trimmed.is_empty() {
        let p = PathBuf::from(trimmed);
        if !p.is_dir() {
            return Err(anyhow!("工作区目录无效或不存在: {trimmed}"));
        }
        return p
            .canonicalize()
            .map(normalize_terminal_cwd)
            .map_err(|e| anyhow!("无法解析工作区路径: {e}"));
    }
    Err(anyhow!(
        "未设置工作区：terminal 不会使用进程 cwd 作为默认工作目录"
    ))
}

/// `canonicalize()` on Windows may return extended-length paths (`\\?\C:\...`).
/// `cmd.exe` rejects those as `WorkingDirectory` — strip before spawning shells.
#[cfg(windows)]
fn normalize_terminal_cwd(path: PathBuf) -> PathBuf {
    PathBuf::from(strip_windows_extended_path_prefix(
        path.display().to_string(),
    ))
}

#[cfg(not(windows))]
fn normalize_terminal_cwd(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
pub(crate) fn strip_windows_extended_path_prefix(path: String) -> String {
    const UNC_PREFIX: &str = "\\\\?\\UNC\\";
    if let Some(rest) = path.strip_prefix(UNC_PREFIX) {
        return format!("\\\\{rest}");
    }
    path.strip_prefix("\\\\?\\")
        .map(str::to_string)
        .unwrap_or(path)
}

/// 超时或需要强制结束时：在 Windows 上仅 `Child::kill` 往往只杀掉 shell（如 PowerShell），
/// 由其拉起的子进程会继续跑；用 `taskkill /T` 结束整棵进程树。其他平台仍用 `kill`。
pub(crate) fn kill_terminal_child_tree_best_effort(child: &mut std::process::Child) {
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
    let session_workspace = crate::tools::file::workspace_root_from_override_or_settings();
    let cwd = effective_terminal_cwd(parse_terminal_cwd(args.get("cwd"))?, &session_workspace)?;
    let max_output_bytes = resolve_max_output_bytes(&args);
    let (shell, _) = terminal_shell_command(&command);
    let env_files = resolve_terminal_env_files(&args, Some(cwd.as_path()))?;

    let r = run_terminal_command_streaming(args, session_workspace, |_| {}, None, None, None)?;

    Ok(serde_json::json!({
        "command": command.as_str(),
        "cwd": cwd.display().to_string(),
        "envFiles": env_files,
        "shell": shell,
        "exitCode": r.exit_code,
        "success": r.success,
        "timedOut": r.timed_out,
        "cancelled": r.cancelled,
        "runAborted": r.run_aborted,
        "needsInputLikely": r.needs_input_likely,
        "inputHint": r.input_hint,
        "inputClass": r.input_class,
        "agentRetryForbidden": r.agent_retry_forbidden,
        "userInputProvided": r.user_input_provided,
        "inputDismissed": r.input_dismissed,
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
    /// User denied OS elevation (UAC / admin password / polkit).
    pub elevation_denied: bool,
    pub needs_input_likely: bool,
    pub input_hint: Option<String>,
    pub input_class: InputClass,
    pub agent_retry_forbidden: bool,
    pub user_input_provided: bool,
    pub input_dismissed: bool,
    pub waited_for_input_ms: u64,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Run a terminal command with optional cooperative cancel.
/// - `cancel`: whole turn stopped (e.g. user "stop generation").
/// - `run_abort`: only this subprocess should stop (`Arc<AtomicBool>` set by host).
/// - `input_hooks`: UI popup path for interactive stdin (streaming dispatch only).
pub fn run_terminal_command_streaming(
    args: serde_json::Value,
    session_workspace: String,
    on_output: impl Fn(&str) + Send,
    cancel: Option<CancellationToken>,
    run_abort: Option<Arc<AtomicBool>>,
    input_hooks: Option<TerminalInputHooks>,
) -> Result<TerminalStreamingResult> {
    // Server-only optional guard (pointer-server.toml). Desktop leaves this off.
    // Workspace Console PTY is a separate path and is never checked here.
    if crate::server_config::forbid_session_user_id_in_terminal() {
        reject_session_user_id_keyword_in_terminal_args(&args)?;
    }

    if terminal_requests_elevation(&args) {
        return run_terminal_command_elevated(
            args,
            session_workspace,
            on_output,
            cancel,
            run_abort,
        );
    }

    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("缺少 command"))?;
    let cwd = effective_terminal_cwd(parse_terminal_cwd(args.get("cwd"))?, &session_workspace)?;
    let timeout_ms = resolve_timeout_ms(&args);
    let wall_cap_ms = resolve_max_wall_ms(&args);
    let max_output_bytes = resolve_max_output_bytes(&args);
    info!(
        "terminal: cwd={} max_output_bytes={}",
        cwd.display(),
        max_output_bytes
    );

    let wait_for_input_ms = args
        .get("waitForInputMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(TERMINAL_DEFAULT_WAIT_FOR_INPUT_MS)
        .clamp(1_000, TERMINAL_MAX_WAIT_FOR_INPUT_MS);

    let env_file_paths = resolve_terminal_env_files(&args, Some(cwd.as_path()))?;

    crate::shell_env::refresh_process_path_from_registry();

    let paths: Vec<PathBuf> = env_file_paths.iter().map(PathBuf::from).collect();
    let stdin_text = args
        .get("stdin")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let use_pty = input_hooks.is_some() && command_wants_pty(command);
    let (tx, rx) = mpsc::channel::<TerminalPipeChunk>();
    let mut input_sink: Option<TerminalInputSink> = None;
    let mut stdin_piped = false;
    #[cfg(unix)]
    let mut askpass_bridge: Option<super::terminal_askpass::AskpassBridge> = None;
    #[cfg(not(unix))]
    let ssh_askpass_active = false;
    #[cfg(unix)]
    let mut ssh_askpass_active = false;

    let mut using_pty = false;

    let pty_spawn = if use_pty {
        #[cfg(unix)]
        let mut pty_askpass_pairs: Option<Vec<(String, String)>> = None;
        #[cfg(unix)]
        {
            if let Some((bridge, helper)) = try_create_ssh_askpass(command, input_hooks.is_some()) {
                pty_askpass_pairs = Some(bridge.env_pairs(&helper));
                askpass_bridge = Some(bridge);
                ssh_askpass_active = true;
                info!("terminal askpass: enabled for OpenSSH PTY session");
            }
        }
        #[cfg(unix)]
        let askpass_ref = pty_askpass_pairs.as_deref();
        #[cfg(not(unix))]
        let askpass_ref: Option<&[(String, String)]> = None;

        match try_spawn_terminal_pty(command, &cwd, &paths, askpass_ref) {
            Ok((pty_child, sink, pty_rx)) => {
                let tx_bridge = tx.clone();
                thread::spawn(move || {
                    while let Ok(text) = pty_rx.recv() {
                        let _ = tx_bridge.send(TerminalPipeChunk {
                            pipe: TerminalPipe::Stdout,
                            text,
                        });
                    }
                });
                input_sink = Some(sink);
                stdin_piped = true;
                using_pty = true;
                Some(ActiveChild::Pty(pty_child))
            }
            Err(e) => {
                warn!("terminal pty: spawn failed, falling back to pipe: {e:#}");
                #[cfg(unix)]
                {
                    askpass_bridge = None;
                    ssh_askpass_active = false;
                }
                None
            }
        }
    } else {
        None
    };

    let mut child = if let Some(pty_child) = pty_spawn {
        pty_child
    } else {
        let want_stdin_pipe = stdin_text.is_some();
        let (_shell, mut cmd) = terminal_shell_command(command);
        cmd.current_dir(&cwd);
        apply_supplemental_env_files(&mut cmd, &paths);
        stdin_piped = want_stdin_pipe;
        cmd.stdin(if want_stdin_pipe {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        #[cfg(unix)]
        {
            askpass_bridge = try_setup_ssh_askpass(command, input_hooks.is_some(), &mut cmd);
            ssh_askpass_active = askpass_bridge.is_some();
        }
        let mut process = cmd.spawn().map_err(|e| anyhow!("启动终端命令失败: {e}"))?;
        if want_stdin_pipe {
            if let Some(stdin) = process.stdin.take() {
                input_sink = Some(TerminalInputSink::Pipe(stdin));
            }
        }
        let stdout_pipe = process
            .stdout
            .take()
            .ok_or_else(|| anyhow!("无法获取 stdout"))?;
        let stderr_pipe = process
            .stderr
            .take()
            .ok_or_else(|| anyhow!("无法获取 stderr"))?;
        spawn_pipe_reader(stdout_pipe, TerminalPipe::Stdout, tx.clone());
        spawn_pipe_reader(stderr_pipe, TerminalPipe::Stderr, tx);
        ActiveChild::Process(process)
    };

    if let Some(text) = stdin_text {
        if let Some(sink) = input_sink.as_mut() {
            sink.write_line(text, true)?;
        }
    }

    let started = Instant::now();
    let mut last_output_at = started;
    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();
    let mut timed_out = false;
    let mut cancelled = false;
    let mut run_aborted = false;
    let mut needs_input_likely = false;
    let mut input_hint: Option<String> = None;
    let mut input_class = InputClass::Normal;
    let mut user_input_provided = false;
    let mut input_dismissed = false;
    let mut waited_for_input_ms: u64 = 0;
    let mut last_secret_submitted: Option<String> = None;

    let status = 'main: loop {
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
            child.kill_best_effort();
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已因会话取消被终止\n");
            break status;
        }

        if run_abort.as_ref().is_some_and(|a| a.load(Ordering::SeqCst)) {
            run_aborted = true;
            warn!("terminal: run-only abort; killing child process tree");
            child.kill_best_effort();
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已由宿主仅终止当前终端命令\n");
            break status;
        }

        #[cfg(unix)]
        if let Some(ref bridge) = askpass_bridge {
            if let Some(askpass_stream) = bridge.try_accept_connection() {
                needs_input_likely = true;
                input_class = InputClass::Secret;
                input_hint = Some("SSH password".to_string());
                if let Some(hooks) = &input_hooks {
                    info!("terminal: SSH_ASKPASS connected; waiting for password in modal");
                    let wait_started = Instant::now();
                    let wait_deadline = wait_started + Duration::from_millis(wait_for_input_ms);
                    let combined = format!("{}{}", stdout_buf, stderr_buf);
                    let wait_prompt = TerminalNeedsInputPrompt {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        command: command.to_string(),
                        output_context: input_context_snippet(&combined),
                        input_hint: Some("SSH password".to_string()),
                        input_class: InputClass::Secret,
                        wait_for_input_ms,
                    };
                    if hooks.prompt_tx.send(wait_prompt).is_err() {
                        warn!("terminal: askpass prompt channel closed");
                    }

                    let mut askpass_stream = Some(askpass_stream);

                    loop {
                        while let Ok(chunk) = rx.try_recv() {
                            on_output(&chunk.text);
                            match chunk.pipe {
                                TerminalPipe::Stdout => stdout_buf.push_str(&chunk.text),
                                TerminalPipe::Stderr => stderr_buf.push_str(&chunk.text),
                            }
                        }

                        if let Some(status) = child.try_wait()? {
                            waited_for_input_ms += wait_started.elapsed().as_millis() as u64;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            break 'main status;
                        }

                        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                            cancelled = true;
                            child.kill_best_effort();
                            let status = child.wait()?;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            on_output("\n进程已因会话取消被终止\n");
                            break 'main status;
                        }

                        if run_abort.as_ref().is_some_and(|a| a.load(Ordering::SeqCst)) {
                            run_aborted = true;
                            child.kill_best_effort();
                            let status = child.wait()?;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            on_output("\n进程已由宿主仅终止当前终端命令\n");
                            break 'main status;
                        }

                        match hooks.resolution_rx.recv_timeout(Duration::from_millis(200)) {
                            Ok(TerminalInputResolution::Submit(text)) => {
                                user_input_provided = true;
                                last_secret_submitted = Some(text.clone());
                                if let Some(stream) = askpass_stream.take() {
                                    if let Err(e) = deliver_askpass_password(stream, &text) {
                                        warn!("terminal: askpass password delivery failed: {e:#}");
                                    }
                                }
                                waited_for_input_ms += wait_started.elapsed().as_millis() as u64;
                                last_output_at = Instant::now();
                                continue 'main;
                            }
                            Ok(TerminalInputResolution::Dismiss) => {
                                input_dismissed = true;
                                timed_out = true;
                                child.kill_best_effort();
                                let status = child.wait()?;
                                drain_pipe_chunks(
                                    &rx,
                                    &mut stdout_buf,
                                    &mut stderr_buf,
                                    &on_output,
                                );
                                on_output("\nSSH 密码输入已取消\n");
                                break 'main status;
                            }
                            Ok(TerminalInputResolution::Cancelled) => {
                                cancelled = true;
                                child.kill_best_effort();
                                let status = child.wait()?;
                                drain_pipe_chunks(
                                    &rx,
                                    &mut stdout_buf,
                                    &mut stderr_buf,
                                    &on_output,
                                );
                                on_output("\n进程已因会话取消被终止\n");
                                break 'main status;
                            }
                            Err(_) => {}
                        }

                        if Instant::now() >= wait_deadline {
                            input_dismissed = true;
                            timed_out = true;
                            child.kill_best_effort();
                            let status = child.wait()?;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            on_output("\nSSH 密码输入超时，已终止执行\n");
                            break 'main status;
                        }

                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        }

        if started.elapsed() >= Duration::from_millis(wall_cap_ms) {
            timed_out = true;
            child.kill_best_effort();
            let status = child.wait()?;
            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
            on_output("\n进程已超时（达到墙钟上限），已终止执行\n");
            break status;
        }

        if last_output_at.elapsed() >= Duration::from_millis(PROMPT_DETECT_IDLE_MS) {
            let combined = format!("{}{}", stdout_buf, stderr_buf);
            let idle_ms = last_output_at.elapsed().as_millis() as u64;
            // Heuristic prompts (password:/y/n/…) after a short settle.
            // Do not proactively force an SSH password modal on first connect —
            // that path was too eager (key auth / hanging connects).
            // Unix: password/passphrase also arrives via SSH_ASKPASS (including PTY).
            // After the user already answered yes/no, fall back to a secret modal
            // when the PTY does not clearly echo password: (Windows / askpass miss).
            let mut prompt = detect_prompt_state(&combined, idle_ms);
            if !prompt.needs_input_likely && !ssh_askpass_active {
                if let Some(fallback) = post_interactive_ssh_secret_prompt(
                    command,
                    &combined,
                    idle_ms,
                    user_input_provided,
                ) {
                    info!("terminal: post-interactive SSH password modal (idle={idle_ms}ms)");
                    prompt = fallback;
                }
            }

            // With ASKPASS_REQUIRE=force, secret prompts are delivered via askpass.
            // Skip heuristic Secret modals to avoid a second popup after askpass.
            let open_heuristic_modal = prompt.needs_input_likely
                && !(ssh_askpass_active && prompt.input_class == InputClass::Secret);

            if prompt.needs_input_likely {
                needs_input_likely = true;
                input_hint = prompt.input_hint.clone();
                input_class = prompt.input_class;
            }

            if open_heuristic_modal {
                if let Some(hooks) = &input_hooks {
                    info!(
                        "terminal: idle prompt detected; waiting for user input class={:?} stdin_piped={stdin_piped}",
                        prompt.input_class
                    );
                    let wait_started = Instant::now();
                    let wait_deadline = wait_started + Duration::from_millis(wait_for_input_ms);
                    let output_len_before_wait = stdout_buf.len() + stderr_buf.len();
                    let request_id = uuid::Uuid::new_v4().to_string();
                    let wait_prompt = TerminalNeedsInputPrompt {
                        request_id: request_id.clone(),
                        command: command.to_string(),
                        output_context: input_context_snippet(&combined),
                        input_hint: prompt.input_hint.clone(),
                        input_class: prompt.input_class,
                        wait_for_input_ms,
                    };
                    if hooks.prompt_tx.send(wait_prompt).is_err() {
                        warn!(
                            "terminal: input prompt channel closed; falling back to host TTY wait"
                        );
                    } else {
                        info!("terminal: input prompt queued request_id={request_id}");
                    }

                    loop {
                        while let Ok(chunk) = rx.try_recv() {
                            on_output(&chunk.text);
                            match chunk.pipe {
                                TerminalPipe::Stdout => stdout_buf.push_str(&chunk.text),
                                TerminalPipe::Stderr => stderr_buf.push_str(&chunk.text),
                            }
                        }

                        if let Some(status) = child.try_wait()? {
                            waited_for_input_ms += wait_started.elapsed().as_millis() as u64;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            break 'main status;
                        }

                        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                            cancelled = true;
                            child.kill_best_effort();
                            let status = child.wait()?;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            on_output("\n进程已因会话取消被终止\n");
                            break 'main status;
                        }

                        if run_abort.as_ref().is_some_and(|a| a.load(Ordering::SeqCst)) {
                            run_aborted = true;
                            child.kill_best_effort();
                            let status = child.wait()?;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            on_output("\n进程已由宿主仅终止当前终端命令\n");
                            break 'main status;
                        }

                        // Host TTY: new output after the prompt usually means the user typed in the dev terminal.
                        if !stdin_piped {
                            let output_len_now = stdout_buf.len() + stderr_buf.len();
                            if output_len_now > output_len_before_wait {
                                waited_for_input_ms += wait_started.elapsed().as_millis() as u64;
                                last_output_at = Instant::now();
                                continue 'main;
                            }
                        }

                        match hooks.resolution_rx.recv_timeout(Duration::from_millis(200)) {
                            Ok(TerminalInputResolution::Submit(text)) => {
                                user_input_provided = true;
                                if prompt.input_class == InputClass::Secret {
                                    last_secret_submitted = Some(text.clone());
                                }
                                if stdin_piped {
                                    if let Some(sink) = input_sink.as_mut() {
                                        sink.write_line(&text, true)?;
                                    }
                                } else {
                                    warn!(
                                        "terminal: modal input ignored (host TTY inherit); use the dev terminal"
                                    );
                                }
                                waited_for_input_ms += wait_started.elapsed().as_millis() as u64;
                                last_output_at = Instant::now();
                                continue 'main;
                            }
                            Ok(TerminalInputResolution::Dismiss) => {
                                input_dismissed = true;
                                timed_out = true;
                                child.kill_best_effort();
                                let status = child.wait()?;
                                drain_pipe_chunks(
                                    &rx,
                                    &mut stdout_buf,
                                    &mut stderr_buf,
                                    &on_output,
                                );
                                on_output("\n命令等待输入已取消\n");
                                break 'main status;
                            }
                            Ok(TerminalInputResolution::Cancelled) => {
                                cancelled = true;
                                child.kill_best_effort();
                                let status = child.wait()?;
                                drain_pipe_chunks(
                                    &rx,
                                    &mut stdout_buf,
                                    &mut stderr_buf,
                                    &on_output,
                                );
                                on_output("\n进程已因会话取消被终止\n");
                                break 'main status;
                            }
                            Err(_) => {}
                        }

                        if Instant::now() >= wait_deadline {
                            input_dismissed = true;
                            timed_out = true;
                            child.kill_best_effort();
                            let status = child.wait()?;
                            drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                            on_output("\n进程等待输入超时，已终止执行\n");
                            break 'main status;
                        }

                        thread::sleep(Duration::from_millis(50));
                    }
                }
            } else if last_output_at.elapsed() >= Duration::from_millis(timeout_ms)
                && !using_pty
                && !ssh_askpass_active
            {
                timed_out = true;
                child.kill_best_effort();
                let status = child.wait()?;
                drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);
                on_output("\n进程已超时（长时间无输出），已终止执行\n");
                break status;
            }
        }

        thread::sleep(Duration::from_millis(50));
    };

    drain_pipe_chunks(&rx, &mut stdout_buf, &mut stderr_buf, &on_output);

    let duration_ms = started.elapsed().as_millis() as u64;
    if input_class == InputClass::Secret {
        if let Some(ref secret) = last_secret_submitted {
            stdout_buf = scrub_secret_echo(&stdout_buf, secret);
            stderr_buf = scrub_secret_echo(&stderr_buf, secret);
        }
    }
    let (stdout, stdout_truncated) = truncate_output(stdout_buf.as_bytes(), max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(stderr_buf.as_bytes(), max_output_bytes);
    let agent_retry_forbidden = agent_retry_forbidden(input_class);

    Ok(TerminalStreamingResult {
        exit_code: status.code(),
        success: status.success() && !timed_out && !cancelled && !run_aborted,
        timed_out,
        cancelled,
        run_aborted,
        elevation_denied: false,
        needs_input_likely,
        input_hint,
        input_class,
        agent_retry_forbidden,
        user_input_provided,
        input_dismissed,
        waited_for_input_ms,
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

struct TerminalPipeReaderState {
    carry: Vec<u8>,
}

impl TerminalPipeReaderState {
    fn push(&mut self, chunk: &[u8]) -> String {
        crate::windows_shell_encoding::decode_utf8_stream(&mut self.carry, chunk)
    }

    fn finish(&mut self) -> String {
        crate::windows_shell_encoding::decode_utf8_finish(&mut self.carry)
    }
}

fn spawn_pipe_reader<R>(reader: R, pipe: TerminalPipe, tx: mpsc::Sender<TerminalPipeChunk>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = reader;
        let mut state = TerminalPipeReaderState { carry: Vec::new() };
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let text = state.push(&buf[..n]);
                    if !text.is_empty() {
                        let _ = tx.send(TerminalPipeChunk { pipe, text });
                    }
                }
                Err(e) => {
                    warn!("terminal: pipe read failed: {e}");
                    break;
                }
            }
        }
        let tail = state.finish();
        if !tail.is_empty() {
            let _ = tx.send(TerminalPipeChunk { pipe, text: tail });
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

fn workspace_root_dir() -> Option<PathBuf> {
    crate::tools::file::resolve_tool_workspace_root().ok()
}

pub(crate) fn resolve_terminal_env_files(
    args: &serde_json::Value,
    cwd: Option<&Path>,
) -> Result<Vec<String>> {
    let raw_paths = parse_env_file_args(args);
    if raw_paths.is_empty() {
        return Ok(default_user_env_file()
            .map(|p| vec![p.display().to_string()])
            .unwrap_or_default());
    }
    let workspace = workspace_root_dir();
    let mut resolved = Vec::new();
    for raw in raw_paths {
        let path = resolve_env_file_path(&raw, cwd, workspace.as_deref())?;
        let display = path.display().to_string();
        if !resolved.contains(&display) {
            resolved.push(display);
        }
    }
    Ok(resolved)
}

pub(crate) fn parse_terminal_cwd(value: Option<&serde_json::Value>) -> Result<Option<PathBuf>> {
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

/// Split a Windows command tail into argv tokens (handles `"…"` and `'…'`).
#[cfg(windows)]
fn split_windows_command_line(s: &str) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_single => {
                in_double = !in_double;
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            ' ' | '\t' if !in_double && !in_single => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// When the model prefixes with `powershell` / `pwsh`, spawn that executable directly.
#[cfg(windows)]
pub(crate) fn parse_direct_powershell_invocation(
    command: &str,
) -> Option<(&'static str, Vec<String>)> {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();
    let (exe, rest) = if lower.starts_with("pwsh.exe") {
        ("pwsh.exe", trimmed.get("pwsh.exe".len()..)?.trim_start())
    } else if lower.starts_with("pwsh ") || lower.starts_with("pwsh/") {
        ("pwsh.exe", trimmed.get(4..)?.trim_start())
    } else if lower.starts_with("powershell.exe") {
        (
            "powershell.exe",
            trimmed.get("powershell.exe".len()..)?.trim_start(),
        )
    } else if lower.starts_with("powershell ") || lower.starts_with("powershell/") {
        ("powershell.exe", trimmed.get(11..)?.trim_start())
    } else {
        return None;
    };
    let args = split_windows_command_line(rest);
    Some((exe, args))
}

/// Strip one pair of matching outer quotes from a `/c` argument string.
#[cfg(windows)]
fn strip_cmd_c_argument_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let open = s.as_bytes()[0];
        let close = s.as_bytes()[s.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

/// When the model already prefixes with `cmd` / `cmd.exe`, spawn **one** `cmd.exe` process.
#[cfg(windows)]
pub(crate) fn parse_direct_cmd_invocation(command: &str) -> Option<(&'static str, String)> {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();
    let rest = if lower.starts_with("cmd.exe") {
        trimmed.get(7..)?.trim_start()
    } else if lower.starts_with("cmd/") {
        trimmed.get(3..)?.trim_start()
    } else if lower.starts_with("cmd ") {
        trimmed.get(3..)?.trim_start()
    } else {
        return None;
    };
    let rest_lower = rest.to_ascii_lowercase();
    let (flag, script_start) = if rest_lower.starts_with("/c") {
        ("/C", rest.get(2..)?.trim_start())
    } else if rest_lower.starts_with("/k") {
        ("/K", rest.get(2..)?.trim_start())
    } else {
        return None;
    };
    Some((flag, strip_cmd_c_argument_quotes(script_start)))
}

/// True when `command` already invokes cmd or PowerShell at the start — legacy fallback via `cmd.exe /C`.
#[cfg(windows)]
pub(crate) fn windows_command_uses_explicit_shell(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "cmd ",
        "cmd.exe",
        "cmd/c",
        "cmd.exe/c",
        "powershell ",
        "powershell.exe",
        "pwsh ",
        "pwsh.exe",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

#[cfg(windows)]
fn terminal_shell_command(command: &str) -> (&'static str, Command) {
    use crate::windows_shell_encoding::{
        prefix_cmd_utf8_codepage, wrap_powershell_args_with_utf8, wrap_powershell_command,
    };

    if let Some((flag, script)) = parse_direct_cmd_invocation(command) {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg(flag).arg(prefix_cmd_utf8_codepage(&script));
        return ("cmd.exe (direct /C)", cmd);
    }
    if let Some((exe, args)) = parse_direct_powershell_invocation(command) {
        let mut cmd = Command::new(exe);
        cmd.args(wrap_powershell_args_with_utf8(args));
        return ("powershell (direct)", cmd);
    }
    if windows_command_uses_explicit_shell(command) {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C").arg(prefix_cmd_utf8_codepage(command));
        return ("cmd.exe /C (explicit shell fallback)", cmd);
    }
    let mut cmd = Command::new("powershell");
    cmd.arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(wrap_powershell_command(command));
    ("powershell -ExecutionPolicy Bypass -Command", cmd)
}

#[cfg(not(windows))]
fn terminal_shell_command(command: &str) -> (&'static str, Command) {
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(command);
    ("sh -lc", cmd)
}

/// Tool-call UI: timeout always fails; otherwise `Some(0)` ⇒ success.
pub fn terminal_stream_tool_status(r: &TerminalStreamingResult) -> (bool, Option<String>) {
    if r.agent_retry_forbidden && r.needs_input_likely && !r.user_input_provided {
        return (
            false,
            Some(
                "需要你在界面输入密码，Agent 不会也无法代填。请重新发起该命令或在弹窗中完成输入。"
                    .to_string(),
            ),
        );
    }
    if r.cancelled {
        return (false, Some("命令已因会话取消被终止".to_string()));
    }
    if r.run_aborted {
        return (
            false,
            Some("命令已由宿主终止（仅结束当前终端）".to_string()),
        );
    }
    if r.elevation_denied {
        return (false, Some("用户已拒绝系统提权".to_string()));
    }
    if r.timed_out {
        if r.needs_input_likely && !r.user_input_provided && !r.agent_retry_forbidden {
            return (
                false,
                Some(
                    "命令可能在等待输入。优先使用非交互参数，或通过 stdin 参数重试（仅非敏感输入）。"
                        .to_string(),
                ),
            );
        }
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

/// Keep the **tail** of a stream when it exceeds `max_bytes` (test summaries, panic
/// locations, and exit lines sit at the end). Prefix a marker; skip UTF-8
/// continuation bytes so the slice does not split a codepoint.
pub(crate) fn truncate_output(bytes: &[u8], max_bytes: usize) -> (String, bool) {
    if bytes.len() <= max_bytes {
        return (String::from_utf8_lossy(bytes).to_string(), false);
    }
    const MARKER: &str = "...[output truncated]\n";
    let keep = max_bytes.saturating_sub(MARKER.len()).max(1);
    let mut start = bytes.len().saturating_sub(keep);
    while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
        start += 1;
    }
    let tail = String::from_utf8_lossy(&bytes[start..]);
    log::info!(
        "terminal: truncated stream from {} bytes to last {} bytes (utf8 start={start})",
        bytes.len(),
        bytes.len().saturating_sub(start)
    );
    (format!("{MARKER}{tail}"), true)
}

/// Host-injected identity. Agent `terminal` must not mention this name in
/// `command` / `stdin` (blocks `SESSION_USER_ID=…` overrides and related probes).
const TERMINAL_FORBIDDEN_SESSION_USER_ID_KEYWORD: &str = "SESSION_USER_ID";

fn reject_session_user_id_keyword_in_terminal_args(args: &serde_json::Value) -> Result<()> {
    let keyword = TERMINAL_FORBIDDEN_SESSION_USER_ID_KEYWORD;
    if let Some(command) = args.get("command").and_then(|v| v.as_str()) {
        if command.contains(keyword) {
            warn!("terminal: rejected command containing {keyword}");
            bail!("terminal 命令不得包含 {keyword}；该变量由 Host 注入，禁止在命令中设置或改写");
        }
    }
    if let Some(stdin) = args.get("stdin").and_then(|v| v.as_str()) {
        if stdin.contains(keyword) {
            warn!("terminal: rejected stdin containing {keyword}");
            bail!("terminal stdin 不得包含 {keyword}；该变量由 Host 注入，禁止通过输入设置或改写");
        }
    }
    Ok(())
}

#[cfg(test)]
mod cwd_tests {
    use super::*;

    #[test]
    fn rejects_session_user_id_keyword_in_command() {
        let err = reject_session_user_id_keyword_in_terminal_args(&serde_json::json!({
            "command": "SESSION_USER_ID=fanwei1 bash -c 'echo hi'"
        }))
        .unwrap_err();
        assert!(err.to_string().contains("SESSION_USER_ID"));
    }

    #[test]
    fn rejects_session_user_id_keyword_in_stdin() {
        let err = reject_session_user_id_keyword_in_terminal_args(&serde_json::json!({
            "command": "bash",
            "stdin": "export SESSION_USER_ID=other\n"
        }))
        .unwrap_err();
        assert!(err.to_string().contains("SESSION_USER_ID"));
    }

    #[test]
    fn truncate_output_keeps_tail_not_head() {
        let body = format!("{}FAILURE_AT_END", "x".repeat(80));
        let (out, truncated) = truncate_output(body.as_bytes(), 40);
        assert!(truncated);
        assert!(out.starts_with("...[output truncated]\n"));
        assert!(out.contains("FAILURE_AT_END"), "tail must be kept: {out}");
        assert!(
            !out.contains(&"x".repeat(40)),
            "head must be dropped: {out}"
        );
    }

    #[test]
    fn truncate_output_under_limit_is_unchanged() {
        let (out, truncated) = truncate_output(b"hello", 40);
        assert!(!truncated);
        assert_eq!(out, "hello");
    }

    #[test]
    fn truncate_output_does_not_split_multibyte_char() {
        let mut body = "x".repeat(80);
        body.push('中');
        body.push_str("TAIL");
        let (out, truncated) = truncate_output(body.as_bytes(), 40);
        assert!(truncated);
        assert!(out.contains("TAIL"));
        assert!(
            out.chars().all(|c| c != '\u{FFFD}'),
            "must not emit replacement char: {out:?}"
        );
    }

    #[test]
    fn resolve_max_output_bytes_tool_arg_cannot_exceed_ceiling() {
        let ceiling = resolve_max_output_bytes(&serde_json::json!({}));
        assert!(ceiling >= crate::models::FLOOR_TERMINAL_OUTPUT_MAX_BYTES as usize);
        assert!(ceiling <= crate::models::CEILING_TERMINAL_OUTPUT_MAX_BYTES as usize);
        let lowered = resolve_max_output_bytes(&serde_json::json!({ "maxOutputBytes": 1024 }));
        assert_eq!(lowered, 1024);
        let raised = resolve_max_output_bytes(&serde_json::json!({
            "maxOutputBytes": u64::from(crate::models::CEILING_TERMINAL_OUTPUT_MAX_BYTES) * 4
        }));
        assert_eq!(raised, ceiling);
    }

    #[test]
    fn resolve_max_wall_ms_tool_arg_cannot_exceed_ceiling() {
        let ceiling = resolve_max_wall_ms(&serde_json::json!({}));
        let floor =
            crate::models::terminal_max_wall_ms(crate::models::FLOOR_TERMINAL_MAX_WALL_HOURS);
        let cap =
            crate::models::terminal_max_wall_ms(crate::models::CEILING_TERMINAL_MAX_WALL_HOURS);
        assert!(ceiling >= floor);
        assert!(ceiling <= cap);
        let lowered = resolve_max_wall_ms(&serde_json::json!({ "maxWallMs": 3_600_000 }));
        assert_eq!(lowered, 3_600_000);
        let raised = resolve_max_wall_ms(&serde_json::json!({ "maxWallMs": u64::MAX }));
        assert_eq!(raised, ceiling);
    }

    #[test]
    fn resolve_timeout_ms_uses_settings_as_default_not_ceiling() {
        let default_ms = resolve_timeout_ms(&serde_json::json!({}));
        let floor =
            crate::models::terminal_timeout_ms(crate::models::FLOOR_TERMINAL_TIMEOUT_SECONDS);
        let cap =
            crate::models::terminal_timeout_ms(crate::models::CEILING_TERMINAL_TIMEOUT_SECONDS);
        assert!(default_ms >= floor);
        assert!(default_ms <= cap);
        let lowered = resolve_timeout_ms(&serde_json::json!({ "timeoutMs": 5_000 }));
        assert_eq!(lowered, 5_000);
        let raised = resolve_timeout_ms(&serde_json::json!({ "timeoutMs": 120_000 }));
        assert_eq!(raised, 120_000);
        let over_hard_cap = resolve_timeout_ms(&serde_json::json!({ "timeoutMs": u64::MAX }));
        assert_eq!(over_hard_cap, cap);
    }

    #[test]
    fn allows_commands_without_session_user_id_keyword() {
        reject_session_user_id_keyword_in_terminal_args(&serde_json::json!({
            "command": "python3 scripts/cwpt_platform_cli.py login",
            "stdin": "ok\n"
        }))
        .unwrap();
    }

    #[test]
    fn effective_terminal_cwd_defaults_to_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().display().to_string();
        let cwd = effective_terminal_cwd(None, &ws).unwrap();
        assert_eq!(cwd, dir.path().canonicalize().unwrap());
    }

    #[test]
    fn effective_terminal_cwd_prefers_explicit_path() {
        let ws_dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let cwd = effective_terminal_cwd(
            Some(other.path().to_path_buf()),
            ws_dir.path().to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(cwd, other.path().canonicalize().unwrap());
    }

    #[test]
    fn effective_terminal_cwd_uses_session_workspace_not_thread_local() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().display().to_string();
        let resolved = std::thread::spawn(move || effective_terminal_cwd(None, &ws).unwrap())
            .join()
            .unwrap();
        let expected = dir.path().canonicalize().unwrap();
        #[cfg(windows)]
        let expected = normalize_terminal_cwd(expected);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn effective_terminal_cwd_errors_without_workspace() {
        let err = effective_terminal_cwd(None, "").unwrap_err().to_string();
        assert!(err.contains("未设置工作区"), "{err}");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn split_windows_command_line_handles_quotes() {
        assert_eq!(
            split_windows_command_line(r#"-Command "choco -v""#),
            vec!["-Command".to_string(), "choco -v".to_string()]
        );
        assert_eq!(
            split_windows_command_line("-NoProfile -Command foo"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "foo".to_string()
            ]
        );
    }

    #[test]
    fn parse_direct_powershell_invocation_extracts_args() {
        assert_eq!(
            parse_direct_powershell_invocation(r#"powershell -Command "choco -v""#),
            Some((
                "powershell.exe",
                vec!["-Command".to_string(), "choco -v".to_string()]
            ))
        );
        assert_eq!(
            parse_direct_powershell_invocation("pwsh -NoProfile -Command $env:Path"),
            Some((
                "pwsh.exe",
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    "$env:Path".to_string()
                ]
            ))
        );
        assert!(parse_direct_powershell_invocation("cmd /c dir").is_none());
    }

    #[test]
    fn parse_direct_cmd_invocation_extracts_script() {
        assert_eq!(
            parse_direct_cmd_invocation(r#"cmd.exe /c "echo %PATH%""#),
            Some(("/C", "echo %PATH%".to_string()))
        );
        assert_eq!(
            parse_direct_cmd_invocation("cmd /c dir"),
            Some(("/C", "dir".to_string()))
        );
        assert_eq!(
            parse_direct_cmd_invocation(r#"cmd.exe /C "choco -v 2>nul || echo NOT_FOUND""#),
            Some(("/C", "choco -v 2>nul || echo NOT_FOUND".to_string()))
        );
        assert!(parse_direct_cmd_invocation("powershell -Command foo").is_none());
    }

    #[test]
    fn windows_explicit_shell_detects_cmd_prefix() {
        assert!(windows_command_uses_explicit_shell(
            r#"cmd.exe /c "python --version""#
        ));
        assert!(windows_command_uses_explicit_shell("cmd /c dir"));
    }

    #[test]
    fn windows_explicit_shell_detects_powershell_prefix() {
        assert!(windows_command_uses_explicit_shell(
            "powershell -Command Get-Location"
        ));
        assert!(windows_command_uses_explicit_shell(
            "pwsh -c $PSVersionTable"
        ));
    }

    #[test]
    fn windows_explicit_shell_false_for_plain_commands() {
        assert!(!windows_command_uses_explicit_shell("python --version"));
        assert!(!windows_command_uses_explicit_shell("npm run build"));
        assert!(!windows_command_uses_explicit_shell("git status"));
    }

    #[test]
    fn strip_windows_extended_path_prefix_removes_verbatim() {
        assert_eq!(
            strip_windows_extended_path_prefix("\\\\?\\C:\\project\\pointer-app".to_string()),
            "C:\\project\\pointer-app"
        );
        assert_eq!(
            strip_windows_extended_path_prefix("\\\\?\\UNC\\server\\share\\repo".to_string()),
            "\\\\server\\share\\repo"
        );
        assert_eq!(
            strip_windows_extended_path_prefix("C:\\already\\normal".to_string()),
            "C:\\already\\normal"
        );
    }
}
