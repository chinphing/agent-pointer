//! Temporary PTY for commands that need a real TTY (SSH password, sudo, expect).
//! One PTY per terminal tool call; destroyed when the command exits.

use crate::dotenv::build_terminal_child_environment;
use anyhow::{anyhow, Result};
use log::{info, warn};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus as StdExitStatus;
use std::sync::mpsc;

#[derive(Debug, Clone)]
pub struct UnifiedExitStatus {
    code: Option<i32>,
    success: bool,
}

impl UnifiedExitStatus {
    pub fn success(&self) -> bool {
        self.success
    }

    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

impl From<StdExitStatus> for UnifiedExitStatus {
    fn from(status: StdExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
        }
    }
}

impl From<portable_pty::ExitStatus> for UnifiedExitStatus {
    fn from(status: portable_pty::ExitStatus) -> Self {
        Self {
            code: Some(status.exit_code() as i32),
            success: status.success(),
        }
    }
}

pub enum ActiveChild {
    Process(std::process::Child),
    Pty(Box<dyn portable_pty::Child + Send + Sync>),
}

impl ActiveChild {
    pub fn try_wait(&mut self) -> Result<Option<UnifiedExitStatus>> {
        match self {
            Self::Process(child) => child
                .try_wait()
                .map(|s| s.map(UnifiedExitStatus::from))
                .map_err(|e| anyhow!("wait failed: {e}")),
            Self::Pty(child) => child
                .try_wait()
                .map(|s| s.map(UnifiedExitStatus::from))
                .map_err(|e| anyhow!("pty wait failed: {e}")),
        }
    }

    pub fn wait(&mut self) -> Result<UnifiedExitStatus> {
        match self {
            Self::Process(child) => child
                .wait()
                .map(UnifiedExitStatus::from)
                .map_err(|e| anyhow!("wait failed: {e}")),
            Self::Pty(child) => child
                .wait()
                .map(UnifiedExitStatus::from)
                .map_err(|e| anyhow!("pty wait failed: {e}")),
        }
    }

    pub fn kill_best_effort(&mut self) {
        match self {
            Self::Process(child) => super::terminal::kill_terminal_child_tree_best_effort(child),
            Self::Pty(child) => {
                if let Err(e) = child.kill() {
                    warn!("terminal pty: kill failed: {e}");
                }
            }
        }
    }
}

/// Commands that should run under a temporary PTY when interactive UI hooks are active.
pub fn command_wants_pty(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    if super::terminal_askpass::command_wants_ssh_askpass(command) {
        return true;
    }
    if lower.contains("sudo ") || lower.starts_with("sudo ") {
        return true;
    }
    if lower.contains(" expect ") || lower.starts_with("expect ") {
        return true;
    }
    false
}

pub enum TerminalInputSink {
    Pipe(std::process::ChildStdin),
    Pty(Box<dyn Write + Send>),
}

impl TerminalInputSink {
    pub fn write_line(&mut self, text: &str, append_newline: bool) -> Result<()> {
        match self {
            Self::Pipe(stdin) => {
                stdin.write_all(text.as_bytes())?;
                if append_newline && !text.ends_with('\n') {
                    stdin.write_all(b"\n")?;
                }
                stdin.flush()?;
            }
            Self::Pty(writer) => {
                writer.write_all(text.as_bytes())?;
                if append_newline && !text.ends_with('\n') && !text.ends_with('\r') {
                    writer.write_all(b"\r")?;
                }
                writer.flush()?;
            }
        }
        Ok(())
    }
}

struct PtyReaderState {
    carry: Vec<u8>,
}

impl PtyReaderState {
    fn push(&mut self, chunk: &[u8]) -> String {
        crate::windows_shell_encoding::decode_utf8_stream(&mut self.carry, chunk)
    }

    fn finish(&mut self) -> String {
        crate::windows_shell_encoding::decode_utf8_finish(&mut self.carry)
    }
}

#[cfg(unix)]
fn shell_command_builder(command: &str) -> CommandBuilder {
    let mut builder = CommandBuilder::new("sh");
    builder.arg("-lc");
    builder.arg(command);
    builder
}

#[cfg(windows)]
fn shell_command_builder(command: &str) -> CommandBuilder {
    use super::terminal::{
        parse_direct_cmd_invocation, parse_direct_powershell_invocation,
        windows_command_uses_explicit_shell,
    };
    use crate::windows_shell_encoding::{
        prefix_cmd_utf8_codepage, wrap_powershell_args_with_utf8, wrap_powershell_command,
    };

    if let Some((flag, script)) = parse_direct_cmd_invocation(command) {
        let mut builder = CommandBuilder::new("cmd.exe");
        builder.arg(flag);
        builder.arg(prefix_cmd_utf8_codepage(&script));
        return builder;
    }
    if let Some((exe, args)) = parse_direct_powershell_invocation(command) {
        let mut builder = CommandBuilder::new(exe);
        for arg in wrap_powershell_args_with_utf8(args) {
            builder.arg(arg);
        }
        return builder;
    }
    if windows_command_uses_explicit_shell(command) {
        let mut builder = CommandBuilder::new("cmd.exe");
        builder.arg("/C");
        builder.arg(prefix_cmd_utf8_codepage(command));
        return builder;
    }
    let mut builder = CommandBuilder::new("powershell");
    builder.arg("-ExecutionPolicy");
    builder.arg("Bypass");
    builder.arg("-Command");
    builder.arg(wrap_powershell_command(command));
    builder
}

fn apply_env_to_builder(builder: &mut CommandBuilder, env_files: &[PathBuf]) {
    for (key, value) in build_terminal_child_environment(env_files) {
        builder.env(key, value);
    }
    builder.env("SSH_ASKPASS", "");
    builder.env("SSH_ASKPASS_REQUIRE", "");
}

fn spawn_pty_reader<R>(mut reader: R, tx: mpsc::Sender<String>)
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut state = PtyReaderState { carry: Vec::new() };
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let text = state.push(&buf[..n]);
                    if !text.is_empty() {
                        let _ = tx.send(text);
                    }
                }
                Err(e) => {
                    warn!("terminal pty: read failed: {e}");
                    break;
                }
            }
        }
        let tail = state.finish();
        if !tail.is_empty() {
            let _ = tx.send(tail);
        }
    });
}

pub fn try_spawn_terminal_pty(
    command: &str,
    cwd: &Path,
    env_files: &[PathBuf],
) -> Result<(
    Box<dyn portable_pty::Child + Send + Sync>,
    TerminalInputSink,
    mpsc::Receiver<String>,
)> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow!("openpty failed: {e}"))?;

    let mut builder = shell_command_builder(command);
    builder.cwd(cwd);
    apply_env_to_builder(&mut builder, env_files);
    builder.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|e| anyhow!("pty spawn failed: {e}"))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow!("pty reader failed: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| anyhow!("pty writer failed: {e}"))?;

    let (tx, rx) = mpsc::channel();
    spawn_pty_reader(reader, tx);
    info!("terminal pty: spawned interactive session");

    Ok((child, TerminalInputSink::Pty(writer), rx))
}

#[cfg(test)]
mod tests {
    use super::command_wants_pty;

    #[test]
    fn pty_commands_include_ssh_and_sudo() {
        assert!(command_wants_pty("ssh user@host"));
        assert!(command_wants_pty("sudo apt update"));
        assert!(command_wants_pty("expect -c 'spawn ssh'"));
        assert!(!command_wants_pty("git status"));
    }
}
