//! local provider 的进程后端。
//!
//! 设计约束（`docs/zh-CN/design/environment-sandbox.md` §3.2 / §4.4）：**只有环境实现内部**
//! 允许构造与结束子进程。`tools/` 层一律通过这里，换后端（Cube / 远端 envd）时只需替换本模块，
//! 不用逐个工具改。
//!
//! 说明：terminal 的 PTY / 提权 / askpass 编排仍在 `tools/terminal_*`，因为那几条路径依赖
//! 交互式 UI 回调；真正换成远端后端时需要 provider 级流式 API（P2 的 `env.exec_streaming`），
//! 届时这些编排会整体下沉，本模块是那个替换点。

use std::ffi::OsStr;
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 构造子进程命令。`tools/` 层不得直接 `Command::new`。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

/// 让子进程成为独立进程组组长（unix），使取消 / 超时能一次终止整棵树。
pub fn configure_process_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
}

/// 隐藏 Windows 控制台窗口（no-op 于其他平台）。
pub fn suppress_console_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// 强制结束整棵进程树。
///
/// Windows 上仅 `Child::kill` 往往只杀掉 shell（如 PowerShell），由其拉起的子进程会继续跑；
/// unix 上 terminal 子进程被放进独立进程组，必须对**进程组**发 `SIGKILL` 才能连同孙进程一起
/// 终止（`sh -lc` 会为管道、后台任务、常驻服务等 fork 出子进程）。
pub fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let pid = child.id();
        if pid > 0 {
            let mut cmd = command("taskkill.exe");
            cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
            suppress_console_window(&mut cmd);
            let _ = cmd.status();
        }
    }
    #[cfg(unix)]
    kill_group(child.id());
    let _ = child.kill();
}

/// `SIGKILL` 整个进程组（组已消失时 `kill(2)` 返回 `ESRCH`，忽略）。
#[cfg(unix)]
pub fn kill_group(pid: u32) {
    if pid == 0 || pid > i32::MAX as u32 {
        return;
    }
    // SAFETY: 负 pid 表示「给整个进程组发信号」，不触碰任何内存。
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub fn kill_group(_pid: u32) {}
