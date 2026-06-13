use super::terminal::{
    effective_terminal_cwd, parse_terminal_cwd, resolve_terminal_env_files, truncate_output,
    TerminalStreamingResult,
};
use crate::dotenv::build_terminal_child_environment;
use anyhow::{anyhow, Context, Result};
use log::info;
#[cfg(unix)]
use log::warn;
use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;
#[cfg(unix)]
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
const WINDOWS_ELEVATION_CANCELLED: i32 = 1223;

pub fn terminal_requests_elevation(args: &serde_json::Value) -> bool {
    args.get("elevated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn run_terminal_command_elevated(
    args: serde_json::Value,
    session_workspace: String,
    on_output: impl Fn(&str) + Send,
    cancel: Option<CancellationToken>,
    run_abort: Option<Arc<AtomicBool>>,
) -> Result<TerminalStreamingResult> {
    if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
        return Ok(cancelled_result(0));
    }
    if run_abort
        .as_ref()
        .is_some_and(|a| a.load(Ordering::SeqCst))
    {
        return Ok(aborted_result(0));
    }

    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("缺少 command"))?;
    let cwd = effective_terminal_cwd(parse_terminal_cwd(args.get("cwd"))?, &session_workspace)?;
    let wall_cap_ms = args
        .get("maxWallMs")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(1_000, 3_600_000))
        .unwrap_or(3_600_000);
    let max_output_bytes = args
        .get("maxOutputBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(20_000)
        .min(200_000) as usize;

    let env_file_paths = resolve_terminal_env_files(&args, Some(cwd.as_path()))?;
    let env_paths: Vec<PathBuf> = env_file_paths.iter().map(PathBuf::from).collect();
    let env = build_terminal_child_environment(&env_paths);

    on_output("[elevated] 提权执行中。请在应用内确认后，在系统权限对话框中授予管理员权限。\n");

    crate::shell_env::refresh_process_path_from_registry();

    let started = Instant::now();
    let inner = run_elevated_platform(command, Some(cwd.as_path()), &env, wall_cap_ms, &on_output)?;
    let duration_ms = started.elapsed().as_millis() as u64;

    if inner.elevation_denied {
        on_output("\n用户已拒绝系统提权（UAC / 管理员密码）\n");
    }

    let (stdout, stdout_truncated) = truncate_output(inner.stdout.as_bytes(), max_output_bytes);
    let (stderr, stderr_truncated) = truncate_output(inner.stderr.as_bytes(), max_output_bytes);

    if !inner.stdout.is_empty() {
        on_output(&inner.stdout);
    }
    if !inner.stderr.is_empty() {
        on_output(&inner.stderr);
    }
    if let Some(code) = inner.exit_code {
        if code != 0 && !inner.elevation_denied {
            on_output(&format!("\n进程退出，退出码: {code}\n"));
        }
    }

    Ok(TerminalStreamingResult {
        exit_code: inner.exit_code,
        success: inner.success,
        timed_out: inner.timed_out,
        cancelled: false,
        run_aborted: false,
        duration_ms,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        elevation_denied: inner.elevation_denied,
    })
}

struct ElevatedPlatformResult {
    exit_code: Option<i32>,
    success: bool,
    timed_out: bool,
    elevation_denied: bool,
    stdout: String,
    stderr: String,
}

#[cfg(windows)]
fn run_elevated_platform(
    command: &str,
    cwd: Option<&Path>,
    env: &HashMap<String, String>,
    _wall_cap_ms: u64,
    on_output: &impl Fn(&str),
) -> Result<ElevatedPlatformResult> {
    use super::terminal::windows_command_uses_explicit_shell;

    let work_dir = cwd
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("无法确定工作目录"))?;

    let temp_dir = std::env::temp_dir().join(format!(
        "pointer-elev-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_dir)?;
    let out_path = temp_dir.join("stdout.txt");
    let err_path = temp_dir.join("stderr.txt");
    let exit_path = temp_dir.join("exit.txt");
    let env_path = temp_dir.join("env.json");
    let job_path = temp_dir.join("job.ps1");
    let launcher_path = temp_dir.join("launcher.ps1");

    let env_json = serde_json::to_string(env)?;
    fs::write(&env_path, env_json)?;

    let command = strip_redundant_windows_elevation(command);
    let command_ps = escape_powershell_single_quoted(&command);
    let work_dir_ps = escape_powershell_single_quoted(&work_dir.display().to_string());
    let out_ps = escape_powershell_single_quoted(&out_path.display().to_string());
    let err_ps = escape_powershell_single_quoted(&err_path.display().to_string());
    let exit_ps = escape_powershell_single_quoted(&exit_path.display().to_string());
    let env_ps = escape_powershell_single_quoted(&env_path.display().to_string());

    let run_block = if windows_command_uses_explicit_shell(&command) {
        format!(
            r#"$p = Start-Process -FilePath 'cmd.exe' -ArgumentList @('/C', {command_ps}) -WorkingDirectory {work_dir_ps} -Wait -NoNewWindow -RedirectStandardOutput {out_ps} -RedirectStandardError {err_ps} -PassThru
if ($null -eq $p) {{ $code = 1 }} else {{ $code = $p.ExitCode }}"#
        )
    } else {
        format!(
            r#"$p = Start-Process -FilePath 'powershell.exe' -ArgumentList @('-ExecutionPolicy','Bypass','-NoProfile','-Command',{command_ps}) -WorkingDirectory {work_dir_ps} -Wait -NoNewWindow -RedirectStandardOutput {out_ps} -RedirectStandardError {err_ps} -PassThru
if ($null -eq $p) {{ $code = 1 }} else {{ $code = $p.ExitCode }}"#
        )
    };

    let job_script = format!(
        r#"$ErrorActionPreference = 'Continue'
Set-Location -LiteralPath {work_dir_ps}
$envMap = Get-Content -LiteralPath {env_ps} -Raw | ConvertFrom-Json
foreach ($p in $envMap.PSObject.Properties) {{
  Set-Item -LiteralPath ("Env:" + $p.Name) -Value ([string]$p.Value)
}}
try {{
  {run_block}
  if ($null -eq $code) {{ $code = 0 }}
}} catch {{
  $_ | Out-File -FilePath {err_ps} -Append -Encoding utf8
  $code = 1
}}
Set-Content -LiteralPath {exit_ps} -Value $code -NoNewline -Encoding ascii
"#
    );
    fs::write(&job_path, job_script)?;

    let job_path_ps = escape_powershell_single_quoted(&job_path.display().to_string());
    let launcher_script = format!(
        r#"$p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList @('-ExecutionPolicy','Bypass','-NoProfile','-File',{job_path_ps})
if ($null -eq $p) {{ exit {cancelled} }}
exit $p.ExitCode
"#,
        cancelled = WINDOWS_ELEVATION_CANCELLED
    );
    fs::write(&launcher_path, launcher_script)?;

    on_output("[elevated] 等待 Windows UAC 确认…\n");

    let launcher_path_str = launcher_path.display().to_string();
    let output = thread::spawn(move || {
        Command::new("powershell.exe")
            .args([
                "-ExecutionPolicy",
                "Bypass",
                "-NoProfile",
                "-File",
                &launcher_path_str,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    })
    .join()
    .map_err(|_| anyhow!("提权启动线程异常"))??;

    if output.status.code() == Some(WINDOWS_ELEVATION_CANCELLED) {
        let _ = fs::remove_dir_all(&temp_dir);
        return Ok(ElevatedPlatformResult {
            exit_code: Some(WINDOWS_ELEVATION_CANCELLED),
            success: false,
            timed_out: false,
            elevation_denied: true,
            stdout: String::new(),
            stderr: String::new(),
        });
    }

    let stdout = read_text_file_best_effort(&out_path);
    let stderr = read_text_file_best_effort(&err_path);
    let exit_code = read_exit_code_file(&exit_path).or_else(|| output.status.code());

    let elevation_denied = exit_code == Some(WINDOWS_ELEVATION_CANCELLED);
    let success = exit_code == Some(0) && !elevation_denied;

    let _ = fs::remove_dir_all(&temp_dir);

    Ok(ElevatedPlatformResult {
        exit_code,
        success,
        timed_out: false,
        elevation_denied,
        stdout,
        stderr,
    })
}

#[cfg(target_os = "macos")]
fn run_elevated_platform(
    command: &str,
    cwd: Option<&Path>,
    env: &HashMap<String, String>,
    wall_cap_ms: u64,
    on_output: &impl Fn(&str),
) -> Result<ElevatedPlatformResult> {
    let command = strip_redundant_sudo(command);
    let temp_dir = std::env::temp_dir().join(format!(
        "pointer-elev-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_dir)?;
    let env_path = temp_dir.join("env.sh");
    write_unix_env_exports_file(&env_path, env)?;
    let script = build_unix_elevated_shell_script(cwd, Some(&env_path), &command);
    let escaped = escape_for_osascript_double_quoted(&script);
    let applescript = format!(r#"do shell script "{escaped}" with administrator privileges"#);

    on_output("[elevated] 等待 macOS 管理员授权…\n");

    let output = run_command_with_wall_cap(
        Command::new("osascript").arg("-e").arg(applescript),
        wall_cap_ms,
    );
    let _ = fs::remove_dir_all(&temp_dir);
    let output = output?;

    let combined_err = String::from_utf8_lossy(&output.stderr).to_string();
    let elevation_denied = combined_err.contains("User canceled")
        || combined_err.contains("-128")
        || output.status.code() == Some(1) && combined_err.to_ascii_lowercase().contains("cancel");

    if elevation_denied {
        return Ok(ElevatedPlatformResult {
            exit_code: Some(1),
            success: false,
            timed_out: false,
            elevation_denied: true,
            stdout: String::new(),
            stderr: combined_err,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(ElevatedPlatformResult {
        exit_code: output.status.code(),
        success: output.status.success(),
        timed_out: false,
        elevation_denied: false,
        stdout,
        stderr: combined_err,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run_elevated_platform(
    command: &str,
    cwd: Option<&Path>,
    env: &HashMap<String, String>,
    wall_cap_ms: u64,
    on_output: &impl Fn(&str),
) -> Result<ElevatedPlatformResult> {
    let command = strip_redundant_sudo(command);
    let temp_dir = std::env::temp_dir().join(format!(
        "pointer-elev-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&temp_dir)?;
    let env_path = temp_dir.join("env.sh");
    write_unix_env_exports_file(&env_path, env)?;
    let script = build_unix_elevated_shell_script(cwd, Some(&env_path), &command);

    on_output("[elevated] 等待 polkit (pkexec) 授权…\n");

    let output = run_command_with_wall_cap(
        Command::new("pkexec")
            .arg("sh")
            .arg("-lc")
            .arg(&script),
        wall_cap_ms,
    );

    let _ = fs::remove_dir_all(&temp_dir);

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            if e.to_string().contains("No such file") {
                return Err(anyhow!(
                    "提权执行需要 pkexec（polkit），当前系统未找到该命令"
                ));
            }
            return Err(e);
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let elevation_denied = output.status.code() == Some(126)
        || stderr.to_ascii_lowercase().contains("dismissed")
        || stderr.to_ascii_lowercase().contains("not authorized");

    if elevation_denied {
        return Ok(ElevatedPlatformResult {
            exit_code: output.status.code(),
            success: false,
            timed_out: false,
            elevation_denied: true,
            stdout: String::new(),
            stderr,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(ElevatedPlatformResult {
        exit_code: output.status.code(),
        success: output.status.success(),
        timed_out: false,
        elevation_denied: false,
        stdout,
        stderr,
    })
}

/// Host already elevates once; strip leading `sudo` so nested prompts are less likely.
#[cfg(unix)]
fn strip_redundant_sudo(command: &str) -> String {
    let mut out = command.trim().to_string();
    let mut stripped = false;
    while out.starts_with("sudo ") {
        out = out["sudo ".len()..].trim_start().to_string();
        stripped = true;
    }
    if stripped {
        info!("terminal elevated: stripped redundant sudo prefix from command");
    }
    out
}

/// Host already elevates via RunAs; unwrap common nested `Start-Process -Verb RunAs` wrappers.
#[cfg(windows)]
fn strip_redundant_windows_elevation(command: &str) -> String {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.contains("start-process") || !lower.contains("runas") {
        return trimmed.to_string();
    }
    if let Some(inner) = extract_runas_inner_command(trimmed) {
        info!("terminal elevated: stripped nested Start-Process -Verb RunAs wrapper");
        return inner;
    }
    trimmed.to_string()
}

#[cfg(windows)]
fn extract_runas_inner_command(command: &str) -> Option<String> {
    let marker = "-verb";
    let lower = command.to_ascii_lowercase();
    let verb_idx = lower.find(marker)?;
    let after_verb = &command[verb_idx + marker.len()..];
    let runas_idx = after_verb.to_ascii_lowercase().find("runas")?;
    let tail = after_verb[runas_idx + "runas".len()..].trim();
    let tail = tail.strip_prefix('-').unwrap_or(tail).trim();
    for key in ["-ArgumentList", "-argumentlist"] {
        if let Some(idx) = tail.to_ascii_lowercase().find(&key.to_ascii_lowercase()) {
            let args = tail[idx + key.len()..].trim();
            let args = args.strip_prefix(':').unwrap_or(args).trim();
            return Some(unquote_windows_arg_list(args));
        }
    }
    None
}

#[cfg(windows)]
fn unquote_windows_arg_list(raw: &str) -> String {
    let raw = raw.trim();
    if (raw.starts_with('\'') && raw.ends_with('\'')) || (raw.starts_with('"') && raw.ends_with('"')) {
        return raw[1..raw.len() - 1].to_string();
    }
    raw.to_string()
}

#[cfg(unix)]
fn build_unix_elevated_shell_script(
    cwd: Option<&Path>,
    env_file: Option<&Path>,
    command: &str,
) -> String {
    let mut parts = Vec::new();
    if let Some(path) = env_file {
        parts.push(format!(
            "set -a && . {} && set +a",
            shell_escape_single_quote(&path.display().to_string())
        ));
    }
    if let Some(dir) = cwd {
        parts.push(format!(
            "cd {}",
            shell_escape_single_quote(&dir.display().to_string())
        ));
    }
    parts.push(command.to_string());
    parts.join(" && ")
}

#[cfg(unix)]
fn write_unix_env_exports_file(path: &Path, env: &HashMap<String, String>) -> Result<()> {
    let mut content = String::new();
    for (key, value) in env {
        content.push_str(&format!(
            "export {}={}\n",
            key,
            shell_escape_single_quote(value)
        ));
    }
    fs::write(path, content).with_context(|| format!("write elevated env file {}", path.display()))
}

#[cfg(unix)]
fn shell_escape_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(windows)]
fn escape_powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(target_os = "macos")]
fn escape_for_osascript_double_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(windows)]
fn read_text_file_best_effort(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    if bytes.is_empty() {
        return String::new();
    }
    // Start-Process redirect files are UTF-8; legacy PowerShell *> may be UTF-16 LE.
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).to_string();
    }
    String::from_utf8_lossy(&bytes).to_string()
}

#[cfg(windows)]
fn read_exit_code_file(path: &Path) -> Option<i32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse().ok()
}

#[cfg(unix)]
fn run_command_with_wall_cap(cmd: &mut Command, wall_cap_ms: u64) -> Result<std::process::Output> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| anyhow!("启动提权命令失败: {e}"))?;
    let started = Instant::now();
    let wall = Duration::from_millis(wall_cap_ms);
    loop {
        if let Some(status) = child.try_wait()? {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_end(&mut stdout);
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_end(&mut stderr);
            }
            return Ok(std::process::Output {
                status,
                stdout,
                stderr,
            });
        }
        if started.elapsed() >= wall {
            let _ = child.kill();
            let status = child.wait()?;
            warn!("terminal elevated: wall clock cap reached; killed child");
            return Ok(std::process::Output {
                status,
                stdout: Vec::new(),
                stderr: b"elevated command timed out (wall clock)".to_vec(),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn cancelled_result(duration_ms: u64) -> TerminalStreamingResult {
    TerminalStreamingResult {
        exit_code: None,
        success: false,
        timed_out: false,
        cancelled: true,
        run_aborted: false,
        duration_ms,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        elevation_denied: false,
    }
}

fn aborted_result(duration_ms: u64) -> TerminalStreamingResult {
    TerminalStreamingResult {
        exit_code: None,
        success: false,
        timed_out: false,
        cancelled: false,
        run_aborted: true,
        duration_ms,
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        elevation_denied: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_requests_elevation_parses_bool() {
        assert!(!terminal_requests_elevation(&serde_json::json!({"command": "ls"})));
        assert!(terminal_requests_elevation(
            &serde_json::json!({"command": "ls", "elevated": true})
        ));
        assert!(!terminal_requests_elevation(
            &serde_json::json!({"command": "ls", "elevated": false})
        ));
    }

    #[test]
    #[cfg(unix)]
    fn unix_shell_escape_single_quote() {
        assert_eq!(shell_escape_single_quote("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    #[cfg(unix)]
    fn strip_redundant_sudo_removes_prefix() {
        assert_eq!(strip_redundant_sudo("sudo apt update"), "apt update");
        assert_eq!(strip_redundant_sudo("sudo sudo ls"), "ls");
        assert_eq!(strip_redundant_sudo("ls -la"), "ls -la");
    }
}
