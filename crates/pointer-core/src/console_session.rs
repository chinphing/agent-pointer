//! Persistent user-controlled PTY sessions for the workspace console.
//!
//! This deliberately stays separate from the Agent `terminal` tool: every
//! console tab owns an interactive shell and may outlive an individual chat turn.

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::models::StreamEvent;
use crate::stream_broadcast::publish_global_stream;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const MAX_ROWS: u16 = 1_000;
const MAX_COLS: u16 = 1_000;

/// Serializable console-tab metadata returned to either frontend host.
#[derive(Debug, Clone, Serialize)]
pub struct ConsoleSessionInfo {
    pub id: String,
    #[serde(rename = "workspaceRoot")]
    pub workspace_root: String,
    pub cwd: String,
    pub label: String,
}

struct ConsoleSession {
    info: ConsoleSessionInfo,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
}

impl ConsoleSession {
    fn emit_output(&self, output: String) {
        if !output.is_empty() {
            publish_global_stream(StreamEvent::ConsoleOutputDelta {
                session_id: self.info.id.clone(),
                workspace_root: self.info.workspace_root.clone(),
                cwd: self.info.cwd.clone(),
                output,
            });
        }
    }
}

/// In-memory registry of independent, user-owned shell tabs.
///
/// Unlike the Agent terminal's one-shot process map, no workspace-level
/// de-duplication occurs: every call to `create` spawns a distinct PTY.
#[derive(Default)]
pub struct ConsoleSessionManager {
    sessions: Mutex<HashMap<String, Arc<ConsoleSession>>>,
}

impl ConsoleSessionManager {
    pub fn create(&self, workspace_root: &str, cwd: Option<&str>, cols: u16, rows: u16) -> Result<ConsoleSessionInfo> {
        let workspace_root = normalize_directory(workspace_root, "工作区目录")?;
        let cwd = match cwd.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => normalize_directory(value, "终端目录")?,
            None => workspace_root.clone(),
        };
        let info = ConsoleSessionInfo {
            id: Uuid::new_v4().to_string(),
            workspace_root,
            label: directory_label(&cwd),
            cwd,
        };
        let session = Arc::new(spawn_console_session(info, cols, rows)?);
        let response = session.info.clone();
        self.sessions.lock().insert(response.id.clone(), session.clone());
        spawn_output_reader(session);
        Ok(response)
    }

    pub fn write(&self, session_id: &str, data: &str) -> Result<()> {
        let session = self.session(session_id)?;
        let mut writer = session.writer.lock();
        writer.write_all(data.as_bytes())?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let session = self.session(session_id)?;
        session.master.lock().resize(normalize_size(cols, rows))?;
        Ok(())
    }

    pub fn close(&self, session_id: &str) -> bool {
        let Some(session) = self.sessions.lock().remove(session_id) else {
            return false;
        };
        if let Err(error) = session.child.lock().kill() {
            log::warn!("console: failed to stop session {session_id}: {error}");
        }
        publish_global_stream(StreamEvent::ConsoleSessionExited {
            session_id: session.info.id.clone(),
            workspace_root: session.info.workspace_root.clone(),
            cwd: session.info.cwd.clone(),
            exit_code: None,
        });
        true
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = self.sessions.lock().keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }

    fn session(&self, session_id: &str) -> Result<Arc<ConsoleSession>> {
        self.sessions
            .lock()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("终端会话不存在或已关闭"))
    }
}

impl Drop for ConsoleSessionManager {
    fn drop(&mut self) {
        let sessions = self.sessions.get_mut();
        for session in sessions.values() {
            if let Err(error) = session.child.lock().kill() {
                log::debug!("console: failed to stop session during cleanup: {error}");
            }
        }
    }
}

fn normalize_directory(input: &str, description: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty() {
        return Err(anyhow!("请先选择{description}"));
    }
    let path = Path::new(value);
    if !path.is_dir() {
        return Err(anyhow!("{description}不存在: {value}"));
    }
    Ok(path.canonicalize()?.to_string_lossy().to_string())
}

fn directory_label(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

fn normalize_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: if cols == 0 { DEFAULT_COLS } else { cols.min(MAX_COLS) },
        rows: if rows == 0 { DEFAULT_ROWS } else { rows.min(MAX_ROWS) },
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn shell_command_builder() -> CommandBuilder {
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|value| Path::new(value).is_file())
            .unwrap_or_else(|| "sh".to_string());
        let mut builder = CommandBuilder::new(shell);
        builder.arg("-l");
        builder
    }
    #[cfg(windows)]
    {
        let mut builder = CommandBuilder::new("powershell");
        builder.arg("-NoLogo");
        builder
    }
}

fn spawn_console_session(info: ConsoleSessionInfo, cols: u16, rows: u16) -> Result<ConsoleSession> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(normalize_size(cols, rows))?;
    let mut builder = shell_command_builder();
    builder.cwd(PathBuf::from(&info.cwd));
    for (key, value) in crate::dotenv::build_terminal_child_environment(&[]) {
        builder.env(key, value);
    }
    builder.env("TERM", "xterm-256color");

    let child = pair.slave.spawn_command(builder)?;
    drop(pair.slave);
    let writer = pair.master.take_writer()?;
    Ok(ConsoleSession {
        info,
        child: Mutex::new(child),
        master: Mutex::new(pair.master),
        writer: Mutex::new(writer),
    })
}

fn spawn_output_reader(session: Arc<ConsoleSession>) {
    std::thread::spawn(move || {
        let reader = session.master.lock().try_clone_reader();
        let Ok(mut reader) = reader else {
            log::warn!("console: failed to clone PTY reader");
            return;
        };
        let mut carry = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(count) => {
                    let output = crate::windows_shell_encoding::decode_utf8_stream(&mut carry, &buf[..count]);
                    session.emit_output(output);
                }
                Err(error) => {
                    log::debug!("console: PTY reader ended: {error}");
                    break;
                }
            }
        }
        let tail = crate::windows_shell_encoding::decode_utf8_finish(&mut carry);
        session.emit_output(tail);
        let exit_code = session.child.lock().wait().ok().map(|status| status.exit_code() as i32);
        publish_global_stream(StreamEvent::ConsoleSessionExited {
            session_id: session.info.id.clone(),
            workspace_root: session.info.workspace_root.clone(),
            cwd: session.info.cwd.clone(),
            exit_code,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::ConsoleSessionManager;

    #[cfg(unix)]
    #[test]
    fn workspace_supports_independent_console_sessions() {
        let root = tempfile::tempdir().expect("workspace");
        let manager = ConsoleSessionManager::default();
        let first = manager
            .create(root.path().to_str().expect("utf8 path"), None, 100, 40)
            .expect("create first shell");
        let second = manager
            .create(root.path().to_str().expect("utf8 path"), None, 80, 24)
            .expect("create second shell");
        assert_ne!(first.id, second.id);
        assert_eq!(first.cwd, second.cwd);
        manager.write(&first.id, "echo first\r").expect("write first");
        manager.resize(&second.id, 120, 50).expect("resize second");
        assert!(manager.close(&first.id));
        manager.write(&second.id, "echo second\r").expect("second remains alive");
        assert!(manager.close(&second.id));
    }
}
