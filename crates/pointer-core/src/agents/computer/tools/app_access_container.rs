//! 容器档位的应用清单与启动。
//!
//! 宿主 `platform::app_access` 走 NSWorkspace / XDG，读的是**宿主**装了什么。
//! 容器会话落到那里就会打开 Mac 上的同名应用——窗口不在容器桌面里，用户和
//! agent 都看不见，还容易误判"启动失败"。所以容器档位单独读容器内的
//! `.desktop`，并就地启动。
//!
//! 只做两件事：列清单、按名字/标识启动。窗口激活、最近使用这类宿主语义
//! 在容器里没有对应物，不假装有。

use crate::env::{Environment, ExecOptions};
use crate::platform::app_access::{AppOpenOptions, AppOpenResult, ListedApp};
use anyhow::Result;
use std::sync::Arc;

/// 扫描顺序：镜像自带的在前，`/etc/pointer/desktop`（dock 用的那套）在后，
/// 用户自己装的排最后。同 id 只保留第一个。
const LIST_SCRIPT: &str = r#"
for d in /usr/share/applications /etc/pointer/desktop "$HOME/.local/share/applications"; do
    [ -d "$d" ] || continue
    for f in "$d"/*.desktop; do
        [ -f "$f" ] || continue
        grep -qi '^NoDisplay=true' "$f" && continue
        name=$(sed -n 's/^Name=//p' "$f" | head -1)
        exec_line=$(sed -n 's/^Exec=//p' "$f" | head -1)
        [ -n "$name" ] || continue
        [ -n "$exec_line" ] || continue
        printf '%s\t%s\t%s\n' "$(basename "$f" .desktop)" "$name" "$exec_line"
    done
done
"#;

/// 枚举当前有窗口的应用。用 `xdotool` + `ps`，镜像里已经有，不必再加 `wmctrl`。
const WINDOW_SCRIPT: &str = r#"
export DISPLAY="${DISPLAY:-:99}"
active=$(xdotool getactivewindow 2>/dev/null || true)
for wid in $(xdotool search --onlyvisible --name "." 2>/dev/null); do
    pid=$(xdotool getwindowpid "$wid" 2>/dev/null || true)
    [ -n "$pid" ] || continue
    comm=$(ps -o comm= -p "$pid" 2>/dev/null | head -1)
    [ -n "$comm" ] || continue
    title=$(xdotool getwindowname "$wid" 2>/dev/null || true)
    act=0
    [ "$wid" = "$active" ] && act=1
    printf '%s\t%s\t%s\t%s\n' "$pid" "$comm" "$act" "$title"
done
"#;

/// 一个可见窗口：`pid<TAB>comm<TAB>active<TAB>title`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    pid: u32,
    comm: String,
    active: bool,
    title: String,
}

fn parse_windows(text: &str) -> Vec<Window> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let pid: u32 = parts.next()?.trim().parse().ok()?;
            let comm = parts.next()?.trim().to_string();
            let active = parts.next()?.trim() == "1";
            let title = parts.next().unwrap_or("").trim().to_string();
            if comm.is_empty() {
                return None;
            }
            Some(Window {
                pid,
                comm,
                active,
                title,
            })
        })
        .collect()
}

/// 哪个窗口属于这个 `.desktop`。
///
/// 按进程名匹配，不按窗口标题：标题是用户内容，随时会变。`comm` 被内核截到
/// 15 字符，所以两边都按前缀比。
fn window_for<'a>(windows: &'a [Window], exec: &str) -> Option<&'a Window> {
    let bin = binary_of(exec).to_lowercase();
    if bin.is_empty() {
        return None;
    }
    windows.iter().find(|w| {
        let comm = w.comm.to_lowercase();
        comm == bin || bin.starts_with(&comm) || comm.starts_with(&bin)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopEntry {
    id: String,
    name: String,
    exec: String,
}

/// 解析 `LIST_SCRIPT` 的输出（`id<TAB>name<TAB>exec`）。纯函数，便于单测。
fn parse_entries(text: &str) -> Vec<DesktopEntry> {
    let mut out: Vec<DesktopEntry> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(id), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let exec = parts.next().unwrap_or("").trim();
        let id = id.trim();
        let name = name.trim();
        if id.is_empty() || name.is_empty() || exec.is_empty() {
            continue;
        }
        if out.iter().any(|e| e.id == id) {
            continue;
        }
        out.push(DesktopEntry {
            id: id.to_string(),
            name: name.to_string(),
            exec: exec.to_string(),
        });
    }
    out
}

/// 去掉 `.desktop` 的 `Exec=` 里的字段码（`%U` `%f` `%i` …）。
///
/// 容器里没有桌面会话去填这些字段，原样传给程序会被当成字面量参数。
fn strip_field_codes(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|tok| !(tok.starts_with('%') && tok.len() == 2))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 从 `Exec=` 里取可执行文件名，用于"用户说的是 binary 名"的情况。
fn binary_of(exec: &str) -> String {
    strip_field_codes(exec)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// 按用户给的字符串挑应用：标识 > 全名 > 子串 > 可执行文件名。
fn pick<'a>(entries: &'a [DesktopEntry], query: &str) -> Option<&'a DesktopEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    entries
        .iter()
        .find(|e| e.id.to_lowercase() == q)
        .or_else(|| entries.iter().find(|e| e.name.to_lowercase() == q))
        .or_else(|| {
            entries
                .iter()
                .find(|e| e.name.to_lowercase().contains(&q) || e.id.to_lowercase().contains(&q))
        })
        .or_else(|| entries.iter().find(|e| binary_of(&e.exec).to_lowercase() == q))
}

fn run(env: &Arc<dyn Environment>, script: &str) -> Result<String> {
    let argv = vec!["sh".to_string(), "-lc".to_string(), script.to_string()];
    let out = env.exec(&argv, ExecOptions::default())?;
    if out.status != 0 {
        anyhow::bail!(
            "容器内命令失败（status={}）：{}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn entries(env: &Arc<dyn Environment>) -> Result<Vec<DesktopEntry>> {
    Ok(parse_entries(&run(env, LIST_SCRIPT)?))
}

/// 容器里装了哪些图形应用。
///
/// `include_all` 在这里没有意义——容器里的 `.desktop` 本来就只有十几个，
/// 不存在宿主那种"只列最近用过的"取舍。
pub fn list(env: &Arc<dyn Environment>, _include_all: bool) -> Result<Vec<ListedApp>> {
    let entries = entries(env)?;
    // 窗口枚举失败不该让整张清单失败：装了什么仍然有用。
    let windows = run(env, WINDOW_SCRIPT)
        .map(|text| parse_windows(&text))
        .unwrap_or_default();

    let mut apps: Vec<ListedApp> = entries
        .into_iter()
        .map(|e| {
            let window = window_for(&windows, &e.exec);
            ListedApp {
                name: e.name,
                identifier: e.id,
                running: window.is_some(),
                frontmost: window.map(|w| w.active).unwrap_or(false),
                last_used: None,
                uses: None,
                window_title: window
                    .map(|w| w.title.clone())
                    .filter(|title| !title.is_empty()),
                pid: window.map(|w| w.pid),
            }
        })
        .collect();
    // 开着的排前面：模型通常想先看"现在有什么"。
    apps.sort_by(|a, b| {
        b.frontmost
            .cmp(&a.frontmost)
            .then(b.running.cmp(&a.running))
            .then(a.name.cmp(&b.name))
    });
    Ok(apps)
}

/// 在容器桌面里启动应用。
///
/// 用 `setsid` 脱离当前 shell：`docker exec` 的管道一关就会给子进程发信号，
/// 不脱离的话浏览器刚起来就被带走。
pub fn launch(
    env: &Arc<dyn Environment>,
    app: &str,
    _options: AppOpenOptions,
) -> Result<AppOpenResult> {
    let all = entries(env)?;
    let Some(entry) = pick(&all, app) else {
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).take(12).collect();
        return Ok(AppOpenResult {
            success: false,
            action: "not_found".to_string(),
            message: format!(
                "容器里没有名为 `{app}` 的应用。可用：{}",
                if names.is_empty() {
                    "（列表为空，镜像可能没装桌面应用）".to_string()
                } else {
                    names.join("、")
                }
            ),
            app_name: None,
        });
    };

    let cmd = strip_field_codes(&entry.exec);
    run(
        env,
        &format!("setsid {cmd} >/dev/null 2>&1 & sleep 0.3"),
    )?;

    Ok(AppOpenResult {
        success: true,
        action: "launched".to_string(),
        message: format!("已在容器桌面启动 {}", entry.name),
        app_name: Some(entry.name.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "chromium\tChromium\tchromium %U\n\
xterm\tXTerm\txterm\n\
plank\tPlank\tplank\n\
noexec\tBad\t\n\
chromium\tChromium 重复\tchromium\n";

    #[test]
    fn parses_tab_separated_listing_and_drops_bad_rows() {
        let entries = parse_entries(SAMPLE);
        // 没有 Exec 的行被丢掉；重复 id 只留第一条。
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].id, "chromium");
        assert_eq!(entries[0].name, "Chromium");
    }

    #[test]
    fn strips_exec_field_codes() {
        assert_eq!(strip_field_codes("chromium %U"), "chromium");
        assert_eq!(
            strip_field_codes("pcmanfm %f --new-window %i"),
            "pcmanfm --new-window"
        );
    }

    /// 真容器跑一遍：列得出镜像里的应用，启动后进程在容器里。
    #[test]
    #[ignore = "需要本机 docker"]
    fn lists_and_launches_inside_a_real_container() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::env::provider::ProviderConfig {
            tier: Some(crate::env::provider::Tier::Container),
            backend: Some(crate::env::provider::Backend::Docker),
            sandbox_url: None,
            volume_host_path: None,
            template: Some("pointer-sandbox-container:latest".to_string()),
            admins: Vec::new(),
            accounts: Default::default(),
            platform: true,
        };
        let env = crate::env::session_env::resolve_session_env(
            &cfg,
            "it-app-access",
            "tester",
            Some(dir.path()),
        )
        .expect("resolve")
        .expect("容器环境");

        let apps = list(&env, true).expect("list");
        assert!(
            apps.iter().any(|a| a.identifier.contains("chromium")),
            "应列出 chromium：{:?}",
            apps.iter().map(|a| &a.identifier).collect::<Vec<_>>()
        );

        let res = launch(&env, "xterm", AppOpenOptions::default()).expect("launch");
        assert!(res.success, "启动失败：{}", res.message);

        // 启动后重新列：xterm 必须是 running（窗口跟踪走 xdotool + ps）。
        std::thread::sleep(std::time::Duration::from_secs(2));
        let after = list(&env, true).expect("list after launch");
        // 镜像里的条目 id 是 `debian-xterm`（Debian 的 desktop 文件名），不是 `xterm`。
        let xterm = after
            .iter()
            .find(|a| a.identifier.contains("xterm"))
            .expect("清单里应有 xterm");
        assert!(xterm.running, "xterm 已启动但仍报 running=false");
        assert!(xterm.window_title.is_some(), "应带上窗口标题");

        std::thread::sleep(std::time::Duration::from_secs(3));
        let argv = vec![
            "sh".to_string(),
            "-lc".to_string(),
            "pgrep -x xterm | head -1".to_string(),
        ];
        let out = env.exec(&argv, ExecOptions::default()).expect("pgrep");
        assert!(
            !String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "容器里没有 xterm 进程"
        );

        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", "pointer-sbx-it-app-access"])
            .output();
    }

    #[test]
    fn parses_window_listing_and_matches_by_process_name() {
        let windows = parse_windows("75\tchromium\t1\tabout:blank - Chromium\n36\tplank\t0\tplank\n");
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].pid, 75);
        assert!(windows[0].active);
        assert_eq!(windows[1].title, "plank");

        // `comm` 被内核截到 15 字符，前缀比才稳。
        let entries = parse_entries(SAMPLE);
        assert_eq!(window_for(&windows, "chromium %U").unwrap().pid, 75);
        assert!(window_for(&windows, "xterm").is_none());
        assert!(window_for(&windows, "plank").is_some());
    }

    #[test]
    fn picks_by_id_name_substring_or_binary() {
        let entries = parse_entries(SAMPLE);
        assert_eq!(pick(&entries, "xterm").unwrap().id, "xterm");
        assert_eq!(pick(&entries, "chromium").unwrap().id, "chromium");
        assert_eq!(pick(&entries, "XTerm").unwrap().id, "xterm");
        assert_eq!(pick(&entries, "chr").unwrap().id, "chromium");
        assert!(pick(&entries, "photoshop").is_none());
    }
}
