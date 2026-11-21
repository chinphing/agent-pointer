//! DockerEnv 真机集成测试（需要本机 docker / OrbStack）。
//!
//! ```bash
//! cargo test -p pointer-core --test docker_env_it -- --ignored --test-threads=1
//! ```

use std::io::Read;
use std::path::Path;

use pointer_core::env::docker::{DockerEnv, DockerSpec};
use pointer_core::env::pathmap::Mount;
use pointer_core::env::{Environment, ExecOptions, PtySpec};

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("info")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn spec(name: &str, workspace: &Path) -> DockerSpec {
    DockerSpec {
        container: name.to_string(),
        image: "pointer-sandbox-headless:latest".to_string(),
        mounts: vec![Mount::new(workspace, "/workspace", false)],
        workdir: "/workspace".into(),
        tier: pointer_core::env::provider::Tier::Headless,
        cpus: None,
        memory_mb: None,
        disk_mb: None,
        vnc_password: None,
    }
}

fn container_spec(name: &str, workspace: &Path) -> DockerSpec {
    DockerSpec {
        container: name.to_string(),
        image: "pointer-sandbox-desktop:latest".to_string(),
        mounts: vec![Mount::new(workspace, "/workspace", false)],
        workdir: "/workspace".into(),
        tier: pointer_core::env::provider::Tier::Desktop,
        cpus: Some(2.0),
        memory_mb: Some(2048),
        disk_mb: None,
        vnc_password: None,
    }
}

struct Guard(DockerEnv);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

#[test]
#[ignore = "需要本机 docker"]
fn docker_env_runs_commands_inside_container() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "from-host").unwrap();
    let env = DockerEnv::create(spec("pointer-it-docker-exec", dir.path())).expect("create");
    let _guard = Guard(env);
    let env = &_guard.0;

    let out = env
        .exec(
            &["/bin/sh".into(), "-c".into(), "pwd; uname -s; cat hello.txt".into()],
            ExecOptions::new(),
        )
        .expect("exec");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status, 0, "stderr={}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("/workspace"), "cwd 应在 /workspace：{text}");
    assert!(text.contains("Linux"), "应在容器内运行：{text}");
    assert!(text.contains("from-host"), "应看到宿主文件（bind 挂载）：{text}");
}

#[test]
#[ignore = "需要本机 docker"]
fn docker_env_host_sees_container_writes() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let env = DockerEnv::create(spec("pointer-it-docker-write", dir.path())).expect("create");
    let _guard = Guard(env);
    let env = &_guard.0;

    env.exec(
        &["/bin/sh".into(), "-c".into(), "echo written-in-container > out.txt".into()],
        ExecOptions::new(),
    )
    .expect("exec");
    let host = std::fs::read_to_string(dir.path().join("out.txt")).expect("host read");
    assert!(host.contains("written-in-container"));
}

#[test]
#[ignore = "需要本机 docker"]
fn docker_env_pty_gets_a_tty() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let env = DockerEnv::create(spec("pointer-it-docker-pty", dir.path())).expect("create");
    let _guard = Guard(env);
    let env = &_guard.0;

    let spec = PtySpec::new(vec![
        "/bin/sh".into(),
        "-c".into(),
        "tty; echo PTY_OK".into(),
    ]);
    let pty = env.pty_open(spec).expect("pty");
    let mut reader = pty.take_reader().expect("reader");
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read");
    assert!(out.contains("/dev/"), "应拿到真实 tty：{out}");
    assert!(out.contains("PTY_OK"), "{out}");
}

#[test]
#[ignore = "需要本机 docker"]
fn desktop_tier_has_desktop_and_can_screenshot() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let env = DockerEnv::create(container_spec("pointer-it-docker-gui", dir.path())).expect("create");
    let _guard = Guard(env);
    let env = &_guard.0;

    assert_eq!(env.kind(), pointer_core::env::EnvKind::Desktop);
    assert!(env.capabilities().desktop);

    // DISPLAY 由 DockerEnv 自动注入，调用方不用自己写。
    let out = env
        .exec(
            &["/bin/sh".into(), "-c".into(), "echo $DISPLAY; xdpyinfo | head -3".into()],
            ExecOptions::new(),
        )
        .expect("exec");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status, 0, "stderr={}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains(":99"), "DISPLAY 应注入 :99：{text}");
    assert!(text.contains("X.Org"), "X 服务应可用：{text}");

    // 截图：能产出真实 PNG，尺寸与虚拟屏一致。
    let shot = env
        .exec(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "import -window root /tmp/shot.png && identify /tmp/shot.png".into(),
            ],
            ExecOptions::new(),
        )
        .expect("screenshot");
    let shot_text = String::from_utf8_lossy(&shot.stdout);
    assert_eq!(shot.status, 0, "stderr={}", String::from_utf8_lossy(&shot.stderr));
    assert!(shot_text.contains("1920x1080"), "截图尺寸不对：{shot_text}");
}

#[test]
#[ignore = "需要本机 docker"]
fn desktop_tier_applies_resource_limits() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let env = DockerEnv::create(container_spec("pointer-it-docker-limits", dir.path())).expect("create");
    let _guard = Guard(env);

    let out = std::process::Command::new("docker")
        .args(["inspect", "-f", "{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}}", "pointer-it-docker-limits"])
        .output()
        .expect("inspect");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("2000000000 "), "CPU 限制没生效：{text}");
    assert!(text.trim().ends_with("2147483648"), "内存限制没生效：{text}");
}

#[test]
#[ignore = "需要本机 docker"]
fn disk_limit_degrades_gracefully_when_unsupported() {
    use pointer_core::env::docker::DockerSpec;

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // macOS（Docker Desktop / OrbStack）不支持 --storage-opt size=，
    // 期望：告警一次、退回不限磁盘、容器照常可用。
    let spec = DockerSpec {
        container: "pointer-it-disk-limit".to_string(),
        image: "pointer-sandbox-headless:latest".to_string(),
        mounts: vec![Mount::new(dir.path(), "/workspace", false)],
        workdir: "/workspace".into(),
        tier: pointer_core::env::provider::Tier::Headless,
        cpus: Some(1.0),
        memory_mb: Some(1024),
        disk_mb: Some(4096),
        vnc_password: None,
    };
    let env = DockerEnv::create(spec).expect("磁盘限制不该导致建容器失败");
    let _guard = Guard(env);
    let out = _guard
        .0
        .exec(
            &["/bin/sh".into(), "-c".into(), "echo alive".into()],
            ExecOptions::new(),
        )
        .expect("exec");
    assert!(String::from_utf8_lossy(&out.stdout).contains("alive"));

    // 内存限制仍然生效（不该被磁盘回退连带丢掉）。
    let inspect = std::process::Command::new("docker")
        .args([
            "inspect",
            "-f",
            "{{.HostConfig.Memory}}",
            "pointer-it-disk-limit",
        ])
        .output()
        .expect("inspect");
    assert_eq!(
        String::from_utf8_lossy(&inspect.stdout).trim(),
        "1073741824"
    );
}

#[test]
#[ignore = "需要本机 docker"]
fn docker_env_pty_reports_exit_code() {
    use pointer_core::env::PtySpec;

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let env = DockerEnv::create(spec("pointer-it-pty-exit", dir.path())).expect("create");
    let _guard = Guard(env);
    let env = &_guard.0;

    let pty = env
        .pty_open(PtySpec::new(vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo done; exit 3".into(),
        ]))
        .expect("pty");

    let mut reader = pty.take_reader().expect("reader");
    let mut out = String::new();
    let _ = reader.read_to_string(&mut out);

    let mut code = None;
    for _ in 0..50 {
        if let Some(c) = pty.try_wait().expect("try_wait") {
            code = Some(c);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(code, Some(3), "沙箱终端必须回报真实退出码，输出={out:?}");
}

#[test]
#[ignore = "需要本机 docker"]
fn docker_env_git_status_and_diff_work() {
    use pointer_core::env::EnvironmentGuard;

    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "{:?}", String::from_utf8_lossy(&out.stderr));
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "t@example.com"]);
    run(&["config", "user.name", "t"]);
    std::fs::write(repo.join("a.txt"), "old\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
    std::fs::write(repo.join("a.txt"), "old\nnew line\n").unwrap();

    let env = std::sync::Arc::new(DockerEnv::create(spec("pointer-it-git", repo)).expect("create"));
    env.exec(
        &[
            "git".into(),
            "config".into(),
            "--global".into(),
            "--add".into(),
            "safe.directory".into(),
            "*".into(),
        ],
        ExecOptions::new(),
    )
    .expect("safe.directory");

    // 变更列表（面板「变更」标签的数据来源）
    let dyn_env: std::sync::Arc<dyn pointer_core::env::Environment> = env.clone();
    let guard = EnvironmentGuard::enter(dyn_env.clone());
    let changes = pointer_core::workspace_read::git_status(dyn_env.as_ref(), repo)
        .expect("git_status");
    assert_eq!(changes.len(), 1, "应有 1 个变更: {changes:?}");
    assert_eq!(changes[0].path, "a.txt");

    let diff = pointer_core::workspace_read::git_diff(dyn_env.as_ref(), repo, "a.txt", None)
        .expect("git_diff");
    let rendered = serde_json::to_string(&diff.diff_lines).unwrap_or_default();
    assert!(
        rendered.contains("new line"),
        "diff 应包含新增行: {rendered}"
    );
    drop(guard);
    let _ = env.destroy();
}

#[test]
#[ignore = "需要本机 docker"]
fn stale_spec_container_is_recreated() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();

    // 模拟"旧契约"留下的容器：名字对、规格标签不对。
    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-it-stale"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let started = std::process::Command::new("docker")
        .args([
            "run", "-d", "--name", "pointer-it-stale",
            "--label", "pointer.spec=stale-from-old-build",
            "pointer-sandbox-headless:latest", "sleep", "infinity",
        ])
        .output()
        .expect("run");
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));

    // 同名容器存在但规格不符 → 必须重建，而不是复用
    let env = DockerEnv::create(spec("pointer-it-stale", dir.path())).expect("create");
    let _guard = Guard(env);

    let label = std::process::Command::new("docker")
        .args([
            "inspect", "-f",
            "{{index .Config.Labels \"pointer.spec\"}}",
            "pointer-it-stale",
        ])
        .output()
        .expect("inspect");
    let label = String::from_utf8_lossy(&label.stdout).trim().to_string();
    assert_ne!(
        label, "stale-from-old-build",
        "旧规格容器没被重建，改了配置也不会生效"
    );
}

/// 配额必须真的落到 `docker run` 上。
///
/// 之前 `cpus/memory_mb/disk_mb` 全是 `None`，单个容器能吃满宿主。
#[test]
#[ignore = "需要本机 docker"]
fn container_gets_resource_limits() {
    if !docker_available() {
        eprintln!("skip: docker 不可用");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = spec("pointer-sbx-it-limits", dir.path());
    cfg.cpus = Some(2.0);
    cfg.memory_mb = Some(1024);
    let _env = DockerEnv::create(cfg).expect("create");

    let out = std::process::Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{.HostConfig.Memory}} {{.HostConfig.NanoCpus}}",
            "pointer-sbx-it-limits",
        ])
        .output()
        .expect("docker inspect");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut parts = text.split_whitespace();
    let memory: u64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let nano: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    assert_eq!(memory, 1024 * 1024 * 1024, "内存上限没生效: {text}");
    assert_eq!(nano, 2_000_000_000, "CPU 上限没生效: {text}");

    let _ = std::process::Command::new("docker")
        .args(["rm", "-f", "pointer-sbx-it-limits"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
