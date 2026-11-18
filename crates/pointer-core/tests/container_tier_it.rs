//! 走**应用真实配置路径**验证 container 档位：`resolve_session_env_for_current`
//! → `acquire_docker` → 容器镜像 / DISPLAY / 桌面工具。
//!
//! ```bash
//! cargo test -p pointer-core --test container_tier_it -- --ignored --test-threads=1
//! ```

use pointer_core::env::provider::{Backend, ProviderConfig, Tier};
use pointer_core::env::session_env::resolve_session_env;
use pointer_core::env::{EnvKind, Environment, ExecOptions};

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
#[ignore = "需要本机 docker"]
fn container_tier_through_provider_config() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };

    let env = resolve_session_env(&cfg, "it-container-tier", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("docker 后端应返回环境");

    assert_eq!(env.kind(), EnvKind::Container);
    assert!(env.capabilities().desktop);

    let out = env
        .exec(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "echo DISPLAY=$DISPLAY; xdpyinfo | head -2; node -v; python3 -V".into(),
            ],
            ExecOptions::new(),
        )
        .expect("exec");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status, 0, "stderr={}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("DISPLAY=:99"), "应注入 DISPLAY：{text}");
    assert!(text.contains("name of display"), "X 服务应可用：{text}");
    assert!(text.contains("v24."), "node 应为 24：{text}");
    assert!(text.contains("Python 3.12"), "python 应为 3.12：{text}");

    // 清理：不留下测试容器。
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-container-tier"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 应用重启后缓存为空时，必须能"收养"已在跑的容器，而不是当作没有桌面。
///
/// 回归：`desktop_env` 只查缓存，重启后返回 None，面板接管入口消失，
/// 要等会话再跑一次工具才恢复。
#[test]
#[ignore = "需要本机 docker"]
fn adopt_existing_container_after_cache_loss() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    // 先按正常路径建出容器。
    resolve_session_env(&cfg, "it-adopt", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("docker 后端应返回环境");

    // 模拟应用重启：缓存里没有这个会话，只能靠收养。
    let adopted = pointer_core::env::session_env::adopt_docker_env(
        &cfg,
        "it-adopt",
        "tester",
        Some(dir.path()),
    )
    .expect("应能收养已在运行的容器");

    assert_eq!(adopted.kind(), EnvKind::Container);
    let endpoint = adopted.desktop_endpoint().expect("收养后应有桌面入口");
    assert!(
        endpoint.ws_url.is_some(),
        "收养后的端点应带 ws 地址：{endpoint:?}"
    );

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-adopt"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 中文输入必须真的进得去。
///
/// 回归：`xdotool type` 只能敲 keymap 里有的键，"腾讯财报0700" 会被静默截成
/// "0700" 且退出码为 0——不报错，模型以为打进去了。
#[test]
#[ignore = "需要本机 docker"]
fn typing_cjk_survives_in_container() {
    use pointer_core::agents::computer::control::ComputerControl;
    use pointer_core::env::{Environment, ExecOptions};

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-typing", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("容器环境");

    // 地址栏是标准的 Ctrl+V 粘贴目标（xterm 要 Ctrl+Shift+V，不适合做判据）。
    env.exec(
        &[
            "sh".to_string(),
            "-lc".to_string(),
            "setsid browser about:blank >/dev/null 2>&1 &".to_string(),
        ],
        ExecOptions::default(),
    )
    .expect("启动浏览器");
    std::thread::sleep(std::time::Duration::from_secs(9));

    let control = pointer_core::agents::computer::control::SandboxControl::new(env.clone());
    control
        .hotkey(&["ctrl".to_string(), "l".to_string()])
        .expect("聚焦地址栏");
    std::thread::sleep(std::time::Duration::from_secs(1));
    control.typing("腾讯财报0700").expect("输入中文");
    std::thread::sleep(std::time::Duration::from_secs(1));
    control
        .hotkey(&["ctrl".to_string(), "a".to_string()])
        .expect("全选");
    control
        .hotkey(&["ctrl".to_string(), "c".to_string()])
        .expect("复制");
    std::thread::sleep(std::time::Duration::from_secs(1));

    let out = env
        .exec(
            &[
                "sh".to_string(),
                "-lc".to_string(),
                "xclip -o -selection clipboard".to_string(),
            ],
            ExecOptions::default(),
        )
        .expect("读剪贴板");
    let typed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        typed.contains("腾讯财报"),
        "地址栏里实际是 {typed:?}，中文没进去"
    );

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-typing"])
        .output();
}

/// 容器还在、但规格变了（换镜像 / 改配额 / 升版本）时，收养必须把它重建，
/// 而不是返回 None 让面板入口消失。
#[test]
#[ignore = "需要本机 docker"]
fn stale_container_is_recreated_on_adopt() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let name = "pointer-sbx-it-stale";

    // 造一个同名容器，但指纹是假的（模拟升级前的旧容器）。
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", name])
        .output();
    let started = std::process::Command::new("docker")
        .args([
            "run", "-d", "--name", name, "--label", "pointer.spec=stale-fingerprint",
            "pointer-sandbox-container:latest", "sleep", "infinity",
        ])
        .output()
        .expect("docker run");
    assert!(started.status.success(), "准备陈旧容器失败");

    let adopted = pointer_core::env::session_env::adopt_docker_env(
        &cfg,
        "it-stale",
        "tester",
        Some(dir.path()),
    )
    .expect("陈旧容器应被重建后返回");

    let label = std::process::Command::new("docker")
        .args([
            "inspect", "-f", "{{index .Config.Labels \"pointer.spec\"}}", name,
        ])
        .output()
        .expect("inspect");
    let label = String::from_utf8_lossy(&label.stdout).trim().to_string();
    assert_ne!(label, "stale-fingerprint", "容器没有被重建：{label}");
    assert!(adopted.desktop_endpoint().is_some());

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", name])
        .output();
}

/// 桌面入口返回的密码必须等于容器里实际生效的密码。
///
/// 密码在 docker run 时注入、容器建好就定死；复用容器时如果重新随机一个，
/// 接管会直接 `password check failed`（应用重启后必然踩到）。
#[test]
#[ignore = "需要本机 docker"]
fn desktop_password_matches_container_env() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-desktop-pw", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("docker 后端应返回环境");

    let endpoint = env.desktop_endpoint().expect("容器档位应有桌面入口");
    let password = endpoint.password.expect("容器档位应有 VNC 密码");

    let inspect = std::process::Command::new("docker")
        .args([
            "inspect",
            "-f",
            "{{range .Config.Env}}{{println .}}{{end}}",
            "pointer-sbx-it-desktop-pw",
        ])
        .output()
        .expect("docker inspect");
    let env_text = String::from_utf8_lossy(&inspect.stdout);
    assert!(
        env_text.contains(&format!("POINTER_VNC_PASSWORD={password}")),
        "端点密码与容器实际密码不一致（接管会认证失败）：\n{env_text}"
    );

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-desktop-pw"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 容器档位的桌面入口必须给出 websockify 的 WS 地址，且端口真的在监听。
#[test]
#[ignore = "需要本机 docker"]
fn desktop_endpoint_exposes_websockify() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-desktop-endpoint", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("docker 后端应返回环境");

    let endpoint = env.desktop_endpoint().expect("容器档位应有桌面入口");
    assert!(
        endpoint.vnc_url.starts_with("vnc://127.0.0.1:"),
        "{}",
        endpoint.vnc_url
    );
    let ws = endpoint.ws_url.expect("应给出 ws 地址");
    assert!(
        ws.starts_with("ws://127.0.0.1:") && ws.ends_with("/websockify"),
        "WS 地址形态不对：{ws}"
    );

    // 等 websockify 起来：docker run -d 立刻返回，入口脚本还要 1s 左右才拉起它。
    // 不能靠 TCP 探活——docker-proxy 在端口映射声明后就接连接，里面没进程也连得上。
    let port: u16 = ws
        .trim_start_matches("ws://127.0.0.1:")
        .split('/')
        .next()
        .and_then(|p| p.parse().ok())
        .expect("解析 WS 端口");
    let mut running = false;
    for _ in 0..40 {
        let ps = env
            .exec(
                &[
                    "/bin/sh".into(),
                    "-c".into(),
                    "pgrep -af websockify | head -1".into(),
                ],
                ExecOptions::new(),
            )
            .expect("pgrep websockify");
        if String::from_utf8_lossy(&ps.stdout).contains("websockify") {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(running, "容器里没起来 websockify");

    // 真的发一次 HTTP 请求：noVNC 的静态文件必须能取到。
    use std::io::{Read, Write};
    let mut body = String::new();
    for _ in 0..10 {
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
            let _ = stream.write_all(b"GET /vnc.html HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
            let mut buf = [0u8; 256];
            if let Ok(n) = stream.read(&mut buf) {
                body = String::from_utf8_lossy(&buf[..n]).to_string();
                if body.contains("200") {
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(
        body.contains("200"),
        "6080 端口没返回 noVNC 页面：{body}"
    );

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-desktop-endpoint"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[test]
#[ignore = "需要本机 docker"]
fn docker_control_drives_the_container_desktop() {
    use pointer_core::agents::computer::control::{ComputerControl, SandboxControl};
    use pointer_core::agents::computer::input::actions::MouseButton;

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-docker-control", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");

    let control = SandboxControl::new(env.clone());

    let (w, h) = control.screen_size().expect("screen_size");
    assert_eq!((w, h), (1920, 1080));

    // 截图与屏幕尺寸同一像素空间。
    let shot = control.screenshot().expect("screenshot");
    assert!(shot.len() > 1000, "截图太小：{} 字节", shot.len());
    assert_eq!(&shot[..2], &[0xFF, 0xD8], "应是 JPEG 头");

    // 鼠标：移动后光标位置应跟过去。
    control.move_to(400, 300).expect("move_to");
    let (cx, cy) = control.cursor_position().expect("cursor");
    assert_eq!((cx, cy), (400, 300));

    // 点击不报错即可（空桌面上没有可点的目标）。
    control.click(MouseButton::Left, Some(500), Some(320), 1).expect("click");

    // 键盘：起一个 xterm 需要装包，这里只验按键不报错。
    control.press("Escape").expect("press");
    control.hotkey(&["ctrl".into(), "a".into()]).expect("hotkey");

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-docker-control"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[test]
#[ignore = "需要本机 docker"]
fn container_has_gui_apps_and_clipboard() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-gui-apps", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");

    // 应用都在
    let out = env
        .exec(
            &["/bin/sh".into(), "-c".into(),
              "command -v chromium; command -v xterm; command -v xclip; command -v browser".into()],
            ExecOptions::new(),
        )
        .expect("exec");
    let text = String::from_utf8_lossy(&out.stdout);
    for app in ["chromium", "xterm", "xclip", "browser"] {
        assert!(text.contains(app), "缺少 {app}：{text}");
    }

    // 浏览器能起来并且窗口出现在 X 里
    env.exec(
        &["/bin/sh".into(), "-c".into(),
          "DISPLAY=:99 browser about:blank >/tmp/browser.log 2>&1 & sleep 8; DISPLAY=:99 xdotool search --name 'about:blank' | head -1".into()],
        ExecOptions::new(),
    )
    .expect("browser");
    let windows = env
        .exec(
            &["/bin/sh".into(), "-c".into(),
              "DISPLAY=:99 xdotool search --class chromium | head -1".into()],
            ExecOptions::new(),
        )
        .expect("search");
    assert_eq!(
        windows.status,
        0,
        "没找到 chromium 窗口：{}",
        String::from_utf8_lossy(&windows.stderr)
    );

    // VNC 必须带密码：容器档位每次建容器都随机生成一个
    let env_var = std::process::Command::new("docker")
        .args([
            "inspect",
            "-f",
            "{{range .Config.Env}}{{println .}}{{end}}",
            "pointer-sbx-it-gui-apps",
        ])
        .output()
        .expect("inspect env");
    let env_text = String::from_utf8_lossy(&env_var.stdout);
    assert!(
        env_text.contains("POINTER_VNC_PASSWORD="),
        "容器没注入 VNC 密码：{env_text}"
    );
    let x11vnc = env
        .exec(
            &["/bin/sh".into(), "-c".into(), "pgrep -a x11vnc | head -1".into()],
            ExecOptions::new(),
        )
        .expect("pgrep x11vnc");
    assert!(
        String::from_utf8_lossy(&x11vnc.stdout).contains("-rfbauth"),
        "x11vnc 没用 -rfbauth 启动：{}",
        String::from_utf8_lossy(&x11vnc.stdout)
    );

    // 容器剪贴板能写能读
    let clip = env
        .exec(
            &["/bin/sh".into(), "-c".into(),
              "printf 'hello-clip' | DISPLAY=:99 xclip -selection clipboard -i; sleep 0.3; DISPLAY=:99 xclip -selection clipboard -o".into()],
            ExecOptions::new(),
        )
        .expect("clipboard");
    assert!(String::from_utf8_lossy(&clip.stdout).contains("hello-clip"));

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-gui-apps"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 真实 GUI 任务：点地址栏 → 输入 URL → 回车 → 页面变色 → 截图能看出变化。
///
/// 这是"桌面控制真的能干活"的判据——前面几个用例只证明单步动作不报错。
#[test]
#[ignore = "需要本机 docker"]
fn gui_task_navigates_and_screenshot_changes() {
    use pointer_core::agents::computer::control::{ComputerControl, SandboxControl};
    use pointer_core::agents::computer::input::actions::MouseButton;

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-gui-task", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");
    let control = SandboxControl::new(env.clone());

    // 1. 起浏览器并等窗口出现
    env.exec(
        &["/bin/sh".into(), "-c".into(),
          "DISPLAY=:99 browser about:blank >/tmp/browser.log 2>&1 &".into()],
        ExecOptions::new(),
    )
    .expect("launch browser");

    let mut ready = false;
    for _ in 0..40 {
        let out = env
            .exec(
                &["/bin/sh".into(), "-c".into(),
                  "DISPLAY=:99 xdotool search --class chromium 2>/dev/null | head -1".into()],
                ExecOptions::new(),
            )
            .expect("search");
        if !String::from_utf8_lossy(&out.stdout).trim().is_empty() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    assert!(ready, "chromium 窗口没起来");
    std::thread::sleep(std::time::Duration::from_secs(2));

    let before = control.screenshot().expect("screenshot before");

    // 2. 点地址栏。窗口位置/尺寸不写死——openbox 规则会把浏览器拉满屏
    //    （profile 记住的旧尺寸会覆盖 --window-size），所以从 X 读实际几何再推算：
    //    地址栏在窗口顶部往下约 70px，水平取窗口中线。
    let geom = env
        .exec(
            &[
                "sh".to_string(),
                "-lc".to_string(),
                "DISPLAY=:99 xdotool getwindowgeometry --shell \
                 $(xdotool search --onlyvisible --class chromium | head -1)"
                    .to_string(),
            ],
            ExecOptions::new(),
        )
        .expect("window geometry");
    let geom = String::from_utf8_lossy(&geom.stdout);
    let field = |key: &str| -> i32 {
        geom.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or_else(|| panic!("几何输出缺 {key}: {geom}"))
    };
    let addr_x = field("X") + field("WIDTH") / 2;
    let addr_y = field("Y") + 70;

    control
        .click(MouseButton::Left, Some(addr_x), Some(addr_y), 1)
        .expect("click address bar");
    std::thread::sleep(std::time::Duration::from_millis(300));

    // 3. 聚焦地址栏并输入 data URL。
    //    先 ctrl+L 再输入：点地址栏这一步验证鼠标能用，但"点完地址栏是否已就绪"
    //    受浏览器初始化时序影响（实测偶发：窗口已出现但工具栏还没响应）。
    //    ctrl+L 是确定性的，避免用例跟着时序飘。
    control.hotkey(&["ctrl".into(), "l".into()]).expect("focus omnibox");
    control.hotkey(&["ctrl".into(), "a".into()]).expect("select all");
    control
        .typing("data:text/html,<body bgcolor=red>POINTER")
        .expect("type url");
    std::thread::sleep(std::time::Duration::from_millis(300));
    control.press("return").expect("press enter");

    // 4. 等页面加载，截图应该明显变红
    let mut red_ratio = 0.0f64;
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let shot = control.screenshot().expect("screenshot after");
        let img = image::load_from_memory(&shot).expect("jpeg").to_rgb8();
        let mut red = 0u64;
        let total = u64::from(img.width()) * u64::from(img.height());
        for pixel in img.pixels() {
            let [r, g, b] = pixel.0;
            if r > 150 && g < 90 && b < 90 {
                red += 1;
            }
        }
        red_ratio = red as f64 / total as f64;
        if red_ratio > 0.2 {
            break;
        }
    }
    assert!(
        red_ratio > 0.2,
        "页面没有变红（红色像素占比 {red_ratio:.3}），导航可能没生效"
    );

    // 截图确实和点击前不同
    let after = control.screenshot().expect("screenshot final");
    assert_ne!(before, after, "截图前后一致，说明界面没变化");

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-gui-task"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 模型看到的画面必须来自容器：容器桌面刷红后，`capture_screen` 的帧应该是红的。
///
/// 这条挡的是"宿主截图 + 容器点击"的静默错位——坐标能对上、内容对不上。
#[test]
#[ignore = "需要本机 docker"]
fn capture_screen_reads_the_container_desktop() {
    use pointer_core::agents::computer::state::ComputerState;
    use pointer_core::env::EnvironmentGuard;

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-capture", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");

    // 把容器桌面刷成红色
    env.exec(
        &["/bin/sh".into(), "-c".into(),
          "DISPLAY=:99 browser 'data:text/html,<body bgcolor=red>' >/tmp/b.log 2>&1 &".into()],
        ExecOptions::new(),
    )
    .expect("launch browser");
    std::thread::sleep(std::time::Duration::from_secs(10));

    // 注意：这里不构造 ComputerState——它会初始化宿主输入后端（macOS 上起一个
    // 永不退出的主线程队列），测试进程会卡在退出阶段。取帧逻辑抽成了
    // `capture_container_frame`，和 `capture_screen` 里用的是同一份。
    let frame = pointer_core::agents::computer::state::ComputerState::capture_container_frame(env.clone())
        .expect("capture_container_frame");

    // 帧尺寸 = 容器虚拟屏，不是宿主显示器
    assert_eq!((frame.width, frame.height), (1920, 1080), "截图应来自容器虚拟屏");

    let img = image::load_from_memory(&frame.jpeg).expect("jpeg").to_rgb8();
    let total = u64::from(img.width()) * u64::from(img.height());
    let mut red = 0u64;
    for pixel in img.pixels() {
        let [r, g, b] = pixel.0;
        if r > 150 && g < 90 && b < 90 {
            red += 1;
        }
    }
    let ratio = red as f64 / total as f64;
    assert!(
        ratio > 0.2,
        "模型看到的帧不是容器桌面（红色占比 {ratio:.3}）——说明还在抓宿主屏"
    );

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-capture"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 容器桌面刚起来时 openbox 没有聚焦窗口，X 会把按键静默丢弃。
/// 键盘动作必须先自己找到（或建立）焦点，否则"打字没反应"。
#[test]
#[ignore = "需要本机 docker"]
fn keyboard_lands_after_focus_is_lost() {
    use pointer_core::agents::computer::control::{ComputerControl, SandboxControl};

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };
    let env = resolve_session_env(&cfg, "it-docker-focus", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");
    let control = SandboxControl::new(env.clone());

    let sh = |script: &str| {
        env.exec(
            &["sh".to_string(), "-lc".to_string(), script.to_string()],
            ExecOptions::new(),
        )
    };

    // 起浏览器：桌面上必须有一个能接收键盘的窗口。
    sh("DISPLAY=:99 nohup /usr/local/bin/browser about:blank >/dev/null 2>&1 & sleep 6").expect("browser");
    let listed = sh("DISPLAY=:99 xdotool search --onlyvisible --name '.*' | wc -l").expect("search");
    assert!(
        String::from_utf8_lossy(&listed.stdout).trim().parse::<u32>().unwrap_or(0) > 0,
        "浏览器没有起来"
    );

    // 确定性制造"没有聚焦窗口"：杀掉窗口管理器。
    // openbox 会立刻把焦点还给窗口，靠 blur 无法稳定复现；没有 WM 时
    // X 的输入焦点默认是 PointerRoot，按键会被丢弃——这正是容器刚起来时的表现。
    sh("pkill -f openbox; sleep 1; true").expect("kill wm");
    // 没有 WM 时 `getactivewindow` 永远失败，用 X 层的输入焦点判定。
    let before = sh("DISPLAY=:99 xdotool getwindowfocus 2>&1 || true").expect("focus before");
    let focus_before = String::from_utf8_lossy(&before.stdout).trim().to_string();

    let shot_before = control.screenshot().expect("shot before");
    // 全程没有点过鼠标：按键必须自己找到焦点才可能落到界面上。
    // 只用 ctrl+L 判定不了：地址栏聚焦在这个 Chromium 版本上不产生可见像素变化
    // （实测差异 0 px），所以再敲一段文字——文字会进地址栏，画面必然变化。
    control.hotkey(&["ctrl".to_string(), "l".to_string()]).expect("hotkey");
    control.typing("focus-probe").expect("type");
    std::thread::sleep(std::time::Duration::from_millis(900));

    let after = sh("DISPLAY=:99 xdotool getwindowfocus 2>&1 || true").expect("focus after");
    let focus_after = String::from_utf8_lossy(&after.stdout).trim().to_string();
    assert!(
        !focus_after.is_empty() && focus_after.chars().all(|c| c.is_ascii_digit()),
        "按键之后仍没有聚焦窗口：{focus_after}"
    );
    assert_ne!(focus_after, focus_before, "焦点没有从根窗口移到应用窗口");

    let shot_after = control.screenshot().expect("shot after");
    assert_ne!(shot_before.len(), shot_after.len(), "画面没有任何变化，按键可能被丢弃");

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-docker-focus"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// 回归：容器重建后浏览器必须还能起来。
///
/// Chromium 在 profile 目录里留 `SingletonLock`（指向旧容器的 hostname）。
/// profile 挂在用户 home 挂载上之后，锁会跨容器存活，新容器里 Chromium 认为
/// "profile 正被另一台机器使用"直接退出——窗口永远不出现。
/// entrypoint 负责在锁属于别的 hostname 时清掉它。
#[test]
#[ignore = "需要本机 docker"]
fn browser_starts_after_container_recreation() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = ProviderConfig {
        tier: Some(Tier::Container),
        backend: Some(Backend::Docker),
        sandbox_url: None,
        volume_host_path: None,
        template: Some("pointer-sandbox-container:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        platform: true,
    };

    let launch = |env: &std::sync::Arc<dyn Environment>| -> usize {
        env.exec(
            &[
                "sh".to_string(),
                "-lc".to_string(),
                "DISPLAY=:99 nohup /usr/local/bin/browser about:blank >/tmp/b.log 2>&1 &".to_string(),
            ],
            ExecOptions::new(),
        )
        .expect("launch browser");
        for _ in 0..40 {
            let out = env
                .exec(
                    &[
                        "sh".to_string(),
                        "-lc".to_string(),
                        "DISPLAY=:99 xdotool search --class chromium 2>/dev/null | wc -l".to_string(),
                    ],
                    ExecOptions::new(),
                )
                .expect("search");
            let n = String::from_utf8_lossy(&out.stdout).trim().parse::<usize>().unwrap_or(0);
            if n > 0 {
                return n;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        0
    };

    // 第一轮：正常起来，并在 profile 里留下 SingletonLock。
    let env = resolve_session_env(&cfg, "it-docker-recreate-a", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");
    assert!(launch(&env) > 0, "第一次启动就没窗口");

    // 模拟容器重建：删掉容器（profile 在宿主挂载上，锁会留下来）。
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-docker-recreate-a"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    // 第二轮：换一个会话（同用户 → 同一个 home 挂载），新容器必须能清掉旧锁。
    let env2 = resolve_session_env(&cfg, "it-docker-recreate-b", "tester", Some(dir.path()))
        .expect("resolve")
        .expect("env");
    assert!(
        launch(&env2) > 0,
        "容器重建后浏览器没起来（profile 的 SingletonLock 没被清理）"
    );

    for name in ["pointer-sbx-it-docker-recreate-a", "pointer-sbx-it-docker-recreate-b"] {
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
