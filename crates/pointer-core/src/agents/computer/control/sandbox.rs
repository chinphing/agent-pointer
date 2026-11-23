//! `ComputerControl` 的容器后端：驱动沙箱里的 X 桌面（desktop 档位）。
//!
//! 宿主侧不碰任何 GUI API——所有动作都翻译成容器内的 `xdotool` 调用，
//! 截图用 ImageMagick 的 `import`。这样"agent 在隔离环境里点鼠标"和
//! "在本机点鼠标"走的是同一套 L1 接口，上层不用分叉。
//!
//! 坐标语义与 [`ComputerControl`] 一致：逻辑像素，和截图同一像素空间。
//! 这里 `xdotool getdisplaygeometry` 与 `import -window root` 都取自同一个
//! X screen，天然对齐。

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

use super::super::input::actions::MouseButton;
use super::ComputerControl;
use crate::env::{Environment, ExecOptions};

/// 通过执行环境自己的 `exec` 驱动那块桌面的控制后端。
///
/// 不绑定 Docker：任何 `capabilities().desktop == true` 的环境（本地容器、
/// 将来的远程沙箱）都走这里；环境差异只在 `exec` 的实现里。
pub struct SandboxControl {
    env: Arc<dyn Environment>,
    display: String,
}

impl SandboxControl {
    pub fn new(env: Arc<dyn Environment>) -> Self {
        Self {
            env,
            display: ":99".to_string(),
        }
    }

    /// 跑一条容器内命令，非零退出码转成错误（带上 stderr）。
    fn run(&self, argv: &[&str]) -> Result<Vec<u8>> {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let out = self.env.exec(&argv, ExecOptions::new())?;
        if out.status != 0 {
            bail!(
                "容器内命令失败（{}）: {}",
                argv.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    }

    fn run_ok(&self, argv: &[&str]) -> Result<()> {
        self.run(argv).map(|_| ())
    }

    /// 确保有一个窗口能接收键盘事件。
    ///
    /// X 把按键发给「当前聚焦窗口」。容器刚起来时 openbox 只有空桌面、没有焦点，
    /// 按键会被静默丢弃——宿主上不会遇到，因为总有一个前台应用。
    fn ensure_focus(&self) -> Result<()> {
        // 已有焦点：不打扰用户/agent 当前操作的那个窗口。
        if self.run(&["xdotool", "getactivewindow"]).is_ok() {
            return Ok(());
        }
        let out = self.run(&["xdotool", "search", "--onlyvisible", "--name", ".*"])?;
        let first = String::from_utf8_lossy(&out)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string);
        // 一个窗口都没有（空桌面）：键盘无处可去，交给调用方，不在这里报错。
        let Some(window) = first else {
            return Ok(());
        };
        // `windowactivate` 依赖窗口管理器（走 _NET_ACTIVE_WINDOW），
        // `windowfocus` 直接调 XSetInputFocus——两者都试，谁通用谁。
        let _ = self.run_ok(&["xdotool", "windowactivate", "--sync", &window]);
        self.run_ok(&["xdotool", "windowfocus", &window])?;
        Ok(())
    }

    /// 键名 → X keysym。对齐 `enigo::parse_key_name` 的命名。
    fn keysym(key: &str) -> Result<String> {
        let lower = key.to_lowercase();
        let mapped = match lower.as_str() {
            "command" | "cmd" | "control" | "ctrl" => "ctrl",
            "meta" | "win" | "windows" | "super" | "lwin" | "rwin" => "super",
            "alt" | "option" | "opt" => "alt",
            "shift" => "shift",
            "return" | "enter" => "Return",
            "tab" => "Tab",
            "space" => "space",
            "escape" | "esc" => "Escape",
            "backspace" => "BackSpace",
            "delete" => "Delete",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "home" => "Home",
            "end" => "End",
            "pageup" => "Prior",
            "pagedown" => "Next",
            other if other.len() == 1 => return Ok(other.to_string()),
            other => bail!("容器桌面不认识的键名: {other}"),
        };
        Ok(mapped.to_string())
    }

    fn button_number(button: MouseButton) -> &'static str {
        match button {
            MouseButton::Left => "1",
            MouseButton::Right => "3",
            MouseButton::Middle => "2",
        }
    }
}

impl ComputerControl for SandboxControl {
    fn move_to(&self, x: i32, y: i32) -> Result<()> {
        self.run_ok(&["xdotool", "mousemove", &x.to_string(), &y.to_string()])
    }

    fn click(
        &self,
        button: MouseButton,
        x: Option<i32>,
        y: Option<i32>,
        clicks: u32,
    ) -> Result<()> {
        if !(1..=2).contains(&clicks) {
            bail!("容器桌面只支持单击/双击（收到 clicks={clicks}）");
        }
        if let (Some(x), Some(y)) = (x, y) {
            self.move_to(x, y)?;
        }
        self.run_ok(&[
            "xdotool",
            "click",
            "--repeat",
            &clicks.to_string(),
            Self::button_number(button),
        ])
    }

    fn mouse_down(&self, button: MouseButton) -> Result<()> {
        self.run_ok(&["xdotool", "mousedown", Self::button_number(button)])
    }

    fn mouse_up(&self, button: MouseButton) -> Result<()> {
        self.run_ok(&["xdotool", "mouseup", Self::button_number(button)])
    }

    fn release_stranded_input(&self) -> Result<()> {
        // 释放所有鼠标键 + 常见修饰键。失败不致命（可能本来就没按住）。
        let _ = self.run_ok(&["xdotool", "mouseup", "1"]);
        let _ = self.run_ok(&["xdotool", "mouseup", "2"]);
        let _ = self.run_ok(&["xdotool", "mouseup", "3"]);
        for key in ["ctrl", "alt", "shift", "super"] {
            let _ = self.run_ok(&["xdotool", "keyup", key]);
        }
        Ok(())
    }

    fn right_click(&self, x: i32, y: i32) -> Result<()> {
        self.click(MouseButton::Right, Some(x), Some(y), 1)
    }

    fn double_click(&self, x: i32, y: i32) -> Result<()> {
        self.click(MouseButton::Left, Some(x), Some(y), 2)
    }

    fn drag_to(&self, x: i32, y: i32) -> Result<()> {
        self.mouse_down(MouseButton::Left)?;
        let moved = self.move_to(x, y);
        let released = self.mouse_up(MouseButton::Left);
        moved.and(released)
    }

    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        // X11 滚轮：4/5 上下，6/7 左右。每格一次 click。
        for _ in 0..dy.unsigned_abs() {
            let button = if dy > 0 { "4" } else { "5" };
            self.run_ok(&["xdotool", "click", button])?;
        }
        for _ in 0..dx.unsigned_abs() {
            let button = if dx > 0 { "7" } else { "6" };
            self.run_ok(&["xdotool", "click", button])?;
        }
        Ok(())
    }

    fn typing(&self, text: &str) -> Result<()> {
        self.ensure_focus()?;
        // `xdotool type` 只能敲当前 keymap 里存在的键：中文、emoji 会被**静默丢弃**
        // （实测 "腾讯财报0700" 只进去 "0700"，退出码还是 0）。非 ASCII 改走
        // 剪贴板粘贴——和 input_at 用的是同一套原语。
        if !text.is_ascii() {
            let previous = self.get_clipboard_text().ok().flatten();
            self.set_clipboard_text(text)?;
            self.run_ok(&["xdotool", "key", "--clearmodifiers", "ctrl+v"])?;
            if let Some(prev) = previous {
                // 给目标应用一点时间读完剪贴板，否则粘出来的是恢复后的旧内容。
                std::thread::sleep(std::time::Duration::from_millis(80));
                let _ = self.set_clipboard_text(&prev);
            }
            return Ok(());
        }
        // `--clearmodifiers` 防止上一个动作残留的修饰键污染输入。
        self.run_ok(&[
            "xdotool",
            "type",
            "--clearmodifiers",
            "--delay",
            "12",
            "--",
            text,
        ])
    }

    fn press(&self, key: &str) -> Result<()> {
        self.ensure_focus()?;
        let keysym = Self::keysym(key)?;
        self.run_ok(&["xdotool", "key", "--clearmodifiers", &keysym])
    }

    fn key_down(&self, key: &str) -> Result<()> {
        self.ensure_focus()?;
        let keysym = Self::keysym(key)?;
        self.run_ok(&["xdotool", "keydown", &keysym])
    }

    fn key_up(&self, key: &str) -> Result<()> {
        self.ensure_focus()?;
        let keysym = Self::keysym(key)?;
        self.run_ok(&["xdotool", "keyup", &keysym])
    }

    fn hotkey(&self, keys: &[String]) -> Result<()> {
        self.ensure_focus()?;
        if keys.is_empty() {
            bail!("hotkey 需要至少一个键");
        }
        let mut combo = Vec::new();
        for key in keys {
            combo.push(Self::keysym(key)?);
        }
        let joined = combo.join("+");
        self.run_ok(&["xdotool", "key", "--clearmodifiers", &joined])
    }

    fn wait(&self, ms: u64) -> Result<()> {
        std::thread::sleep(Duration::from_millis(ms));
        Ok(())
    }

    fn screenshot(&self) -> Result<Vec<u8>> {
        // `import -window root png:-` 把整屏 PNG 写到 stdout。
        let png = self
            .env
            .exec(
                &[
                    "import".into(),
                    "-display".into(),
                    self.display.clone(),
                    "-window".into(),
                    "root".into(),
                    "png:-".into(),
                ],
                ExecOptions {
                    max_output_bytes: Some(64 * 1024 * 1024),
                    ..ExecOptions::new()
                },
            )
            .context("容器截图失败（ImageMagick import）")?;
        if png.status != 0 {
            bail!(
                "容器截图失败: {}",
                String::from_utf8_lossy(&png.stderr).trim()
            );
        }
        let image = image::load_from_memory(&png.stdout).context("截图不是合法 PNG")?;
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82);
        encoder
            .encode_image(&image)
            .map_err(|e| anyhow!("截图转 JPEG 失败: {e}"))?;
        Ok(jpeg)
    }

    fn screen_size(&self) -> Result<(u32, u32)> {
        let out = self.run(&["xdotool", "getdisplaygeometry"])?;
        let text = String::from_utf8_lossy(&out);
        let mut parts = text.split_whitespace();
        let w: u32 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| anyhow!("getdisplaygeometry 输出无法解析: {text}"))?;
        let h: u32 = parts
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| anyhow!("getdisplaygeometry 输出无法解析: {text}"))?;
        Ok((w, h))
    }

    fn set_clipboard_text(&self, text: &str) -> Result<()> {
        // xclip 从 stdin 读，写完要常驻才能把内容留在 X 剪贴板上；
        // `-loops 1` + 后台守护由 xclip 自己处理（读到选择被取走为止）。
        let out = self
            .env
            .exec(
                &[
                    "xclip".into(),
                    "-selection".into(),
                    "clipboard".into(),
                    "-i".into(),
                ],
                ExecOptions {
                    stdin: Some(text.as_bytes().to_vec()),
                    ..ExecOptions::new()
                },
            )
            .context("写容器剪贴板失败（xclip）")?;
        if out.status != 0 {
            bail!(
                "写容器剪贴板失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    fn get_clipboard_text(&self) -> Result<Option<String>> {
        let out = self
            .env
            .exec(
                &[
                    "xclip".into(),
                    "-selection".into(),
                    "clipboard".into(),
                    "-o".into(),
                ],
                ExecOptions::new(),
            )
            .context("读容器剪贴板失败（xclip）")?;
        if out.status != 0 {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&out.stdout).to_string()))
    }

    fn cursor_position(&self) -> Result<(i32, i32)> {
        let out = self.run(&["xdotool", "getmouselocation", "--shell"])?;
        let text = String::from_utf8_lossy(&out);
        let mut x = None;
        let mut y = None;
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("X=") {
                x = v.trim().parse().ok();
            } else if let Some(v) = line.strip_prefix("Y=") {
                y = v.trim().parse().ok();
            }
        }
        match (x, y) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => bail!("getmouselocation 输出无法解析: {text}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SandboxControl;

    #[test]
    fn key_names_map_to_x_keysyms() {
        assert_eq!(SandboxControl::keysym("ctrl").unwrap(), "ctrl");
        assert_eq!(SandboxControl::keysym("Enter").unwrap(), "Return");
        assert_eq!(SandboxControl::keysym("esc").unwrap(), "Escape");
        assert_eq!(SandboxControl::keysym("a").unwrap(), "a");
        assert!(SandboxControl::keysym("nosuchkey").is_err());
    }
}
