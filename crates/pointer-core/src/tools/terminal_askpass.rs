//! OpenSSH `SSH_ASKPASS` bridge for password prompts when there is no TTY
//! (packaged Tauri app, or piped terminal). Unix only — the helper connects
//! back to a Unix socket; we show the input modal and write the password once.

#[cfg(unix)]
use anyhow::{anyhow, Result};
#[cfg(unix)]
use log::{info, warn};
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::sync::mpsc;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

#[cfg(unix)]
const ASKPASS_HELPER: &str = r#"#!/bin/sh
exec python3 -c '
import os, socket, sys
path = os.environ.get("POINTER_ASKPASS_SOCKET", "")
if not path:
    sys.exit(1)
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(path)
data = s.recv(8192)
sys.stdout.write(data.decode("utf-8", errors="replace"))
' 2>/dev/null
"#;

/// True when OpenSSH is likely to need a GUI/askpass password (no BatchMode).
pub fn command_wants_ssh_askpass(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    if !(lower.contains("ssh ") || lower.starts_with("ssh ")
        || lower.starts_with("ssh-copy-id")
        || lower.contains(" ssh-copy-id ")
        || lower.contains(" scp ") || lower.starts_with("scp ")
        || lower.contains(" sftp ") || lower.starts_with("sftp "))
    {
        return false;
    }
    if lower.contains("batchmode=yes") {
        return false;
    }
    true
}

#[cfg(unix)]
pub struct AskpassBridge {
    socket_path: PathBuf,
    connection_rx: mpsc::Receiver<UnixStream>,
    _listener_thread: thread::JoinHandle<()>,
}

#[cfg(unix)]
impl AskpassBridge {
    pub fn start() -> Result<Self> {
        let dir = std::env::temp_dir().join("pointer-askpass");
        std::fs::create_dir_all(&dir)?;
        let socket_path = dir.join(format!("askpass-{}.sock", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| anyhow!("askpass: bind socket failed: {e}"))?;
        let (connection_tx, connection_rx) = mpsc::channel();
        let listener_thread = thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                if connection_tx.send(conn).is_err() {
                    break;
                }
            }
        });
        info!(
            "terminal askpass: listening on {}",
            socket_path.display()
        );
        Ok(Self {
            socket_path,
            connection_rx,
            _listener_thread: listener_thread,
        })
    }

    pub fn apply_to_command(&self, cmd: &mut Command, helper_path: &Path) {
        cmd.env("SSH_ASKPASS", helper_path);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        cmd.env(
            "POINTER_ASKPASS_SOCKET",
            self.socket_path.display().to_string(),
        );
        // OpenSSH still checks DISPLAY on some builds even with ASKPASS_REQUIRE=force.
        cmd.env("DISPLAY", ":0");
    }

    pub fn try_accept_connection(&self) -> Option<UnixStream> {
        self.connection_rx.try_recv().ok()
    }
}

#[cfg(unix)]
pub fn ensure_askpass_helper_script() -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "pointer-ssh-askpass-{}.sh",
        std::process::id()
    ));
    std::fs::write(&path, ASKPASS_HELPER)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

#[cfg(unix)]
pub fn deliver_askpass_password(mut stream: UnixStream, password: &str) -> Result<()> {
    stream
        .write_all(password.as_bytes())
        .map_err(|e| anyhow!("askpass: write password failed: {e}"))?;
    stream
        .flush()
        .map_err(|e| anyhow!("askpass: flush failed: {e}"))?;
    Ok(())
}

#[cfg(unix)]
pub fn try_setup_ssh_askpass(
    command: &str,
    input_hooks_active: bool,
    cmd: &mut Command,
) -> Option<AskpassBridge> {
    if !input_hooks_active || !command_wants_ssh_askpass(command) {
        return None;
    }
    match ensure_askpass_helper_script().and_then(|helper| {
        AskpassBridge::start().map(|bridge| (helper, bridge))
    }) {
        Ok((helper, bridge)) => {
            bridge.apply_to_command(cmd, &helper);
            info!("terminal askpass: enabled for OpenSSH command");
            Some(bridge)
        }
        Err(e) => {
            warn!("terminal askpass: setup failed: {e:#}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::command_wants_ssh_askpass;

    #[test]
    fn ssh_commands_enable_askpass() {
        assert!(command_wants_ssh_askpass("ssh user@host"));
        assert!(command_wants_ssh_askpass("scp file user@host:/tmp"));
        assert!(!command_wants_ssh_askpass("ssh -o BatchMode=yes user@host"));
        assert!(!command_wants_ssh_askpass("git status"));
    }
}
