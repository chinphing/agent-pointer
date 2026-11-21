//! L1 control abstraction: the OSWorld `computer_13` action set plus observation.
//!
//! [`LocalControl`] drives this host directly. A remote implementation (OSWorld
//! VM over HTTP) implements the same trait, so L2 and the tool layer never
//! branch on which backend is in use.
//!
//! # Observation contract
//!
//! `screenshot()` and `screen_size()` must describe the **same** pixel space.
//! Local capture reads physical pixels and resamples to the logical resolution,
//! so both report logical pixels. A backend that breaks this makes every
//! locator result land at a scaled offset.
//!
//! # Coordinates
//!
//! All coordinates in this trait are **logical screen pixels**, matching what
//! the model sees in the screenshot it is given.

use anyhow::Result;

use super::input::actions::MouseButton;

pub mod sandbox;
pub mod local;
pub mod remote;

pub use sandbox::SandboxControl;
pub use local::LocalControl;
pub use remote::{RemoteControl, RemoteTransport};

/// 按会话挑控制后端：desktop 档位走容器桌面，其余走宿主。
///
/// 这一步必须按会话解析——同一个进程里可能一个会话在沙箱里跑、
/// 另一个在宿主跑。`fallback` 是宿主后端（进程级构造的那份）。
pub fn control_for_conversation(
    fallback: &std::sync::Arc<dyn ComputerControl>,
    conversation_id: &str,
) -> std::sync::Arc<dyn ComputerControl> {
    match sandbox_desktop_for(conversation_id) {
        Some(control) => control,
        None => fallback.clone(),
    }
}

/// 会话的桌面如果由**执行环境自己**提供（容器、远程沙箱），返回驱动它的控制后端。
///
/// 判据是 `capabilities().desktop`，不是后端类型：envd 沙箱只要声明有桌面，
/// 这里自动接上，不需要再改一次选路。返回 `None` 表示桌面在宿主上。
pub fn sandbox_desktop_for(conversation_id: &str) -> Option<std::sync::Arc<dyn ComputerControl>> {
    Some(std::sync::Arc::new(SandboxControl::new(
        sandbox_env_for(conversation_id)?,
    )))
}

/// 桌面由执行环境提供时，返回那个环境本身。
///
/// 应用清单/启动、剪贴板这些要按环境读写（`.desktop`、X 选择区），光有控制
/// 后端不够。
pub fn sandbox_env_for(
    conversation_id: &str,
) -> Option<std::sync::Arc<dyn crate::env::Environment>> {
    let env = crate::env::session_env::env_for_conversation(conversation_id)?;
    // local 档位是**宿主**桌面，不是"环境提供的桌面"：
    // `Capabilities::local().desktop` 表示"这台机器有桌面"，把它当成沙箱桌面会让
    // 鼠标键盘工具去宿主上跑 `xdotool`。宿主控制走调用方传的 fallback（LocalControl）。
    if env.kind() == crate::env::EnvKind::Local {
        return None;
    }
    env.capabilities().desktop.then_some(env)
}

/// The OSWorld `computer_13` actions plus screen observation.
pub trait ComputerControl: Send + Sync {
    /// `MOVE_TO` — move the pointer to `(x, y)`.
    fn move_to(&self, x: i32, y: i32) -> Result<()>;

    /// `CLICK` — with coordinates, or at the current pointer when both are `None`.
    ///
    /// `clicks` is 1 or 2. Implementations must reject counts they cannot
    /// express rather than silently collapsing them to 1.
    fn click(
        &self,
        button: MouseButton,
        x: Option<i32>,
        y: Option<i32>,
        clicks: u32,
    ) -> Result<()>;

    /// `MOUSE_DOWN` — press a button at the current pointer.
    fn mouse_down(&self, button: MouseButton) -> Result<()>;

    /// `MOUSE_UP` — release a button at the current pointer.
    fn mouse_up(&self, button: MouseButton) -> Result<()>;

    /// Release every modifier key and mouse button this backend may be holding.
    ///
    /// `MOUSE_DOWN` / `KEY_DOWN` are stateful. Call this when a run ends, is
    /// cancelled, or the process is about to exit — an unpaired press turns
    /// every later click into a modified click and leaves the keyboard stuck.
    fn release_stranded_input(&self) -> Result<()>;

    /// `RIGHT_CLICK` at `(x, y)`.
    fn right_click(&self, x: i32, y: i32) -> Result<()>;

    /// `DOUBLE_CLICK` at `(x, y)`.
    fn double_click(&self, x: i32, y: i32) -> Result<()>;

    /// `DRAG_TO` — drag from the current pointer to `(x, y)`.
    fn drag_to(&self, x: i32, y: i32) -> Result<()>;

    /// `SCROLL` — horizontal `dx`, vertical `dy`.
    fn scroll(&self, dx: i32, dy: i32) -> Result<()>;

    /// `TYPING` — type `text` into the focused widget.
    fn typing(&self, text: &str) -> Result<()>;

    /// `PRESS` — press and release one key.
    fn press(&self, key: &str) -> Result<()>;

    /// `KEY_DOWN` — hold a key.
    fn key_down(&self, key: &str) -> Result<()>;

    /// `KEY_UP` — release a key.
    fn key_up(&self, key: &str) -> Result<()>;

    /// `HOTKEY` — press `keys` in order, release in reverse.
    fn hotkey(&self, keys: &[String]) -> Result<()>;

    /// `WAIT` — terminal signal; sleeps for `ms` milliseconds.
    fn wait(&self, ms: u64) -> Result<()>;

    /// Current screen as JPEG bytes at logical-pixel resolution.
    fn screenshot(&self) -> Result<Vec<u8>>;

    /// Screen size in logical pixels; matches [`screenshot`](Self::screenshot).
    fn screen_size(&self) -> Result<(u32, u32)>;

    /// Pointer position in logical screen pixels (diagnostics only).
    fn cursor_position(&self) -> Result<(i32, i32)>;

    /// 写系统剪贴板。容器后端覆盖成容器内的 X 剪贴板——
    /// 宿主剪贴板和容器剪贴板是两个世界，写错了粘贴出来就是空。
    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        crate::platform::clipboard::write_text(text)
    }

    /// 读系统剪贴板（用于粘贴后恢复原内容）。
    fn get_clipboard_text(&self) -> Result<Option<String>> {
        Ok(crate::platform::clipboard::read_text().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// local 档位是**宿主**桌面：必须交给 fallback（`LocalControl`），
    /// 不能让 `SandboxControl` 去宿主上跑 `xdotool` / `import`。
    #[test]
    fn local_session_is_not_a_sandbox_desktop() {
        let dir = tempfile::tempdir().unwrap();
        crate::env::session_env::remember_local_for_test("conv-local-control", dir.path());
        assert!(
            sandbox_env_for("conv-local-control").is_none(),
            "local 档位不能当成沙箱桌面"
        );
        assert!(sandbox_desktop_for("conv-local-control").is_none());
        crate::env::session_env::release("conv-local-control");
    }

    #[test]
    fn unknown_conversation_has_no_sandbox_desktop() {
        assert!(sandbox_env_for("conv-never-bound").is_none());
    }
}
