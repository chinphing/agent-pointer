//! RemoteEnv 对 e2b-local 的真机集成测试。
//!
//! 需要网关在跑（`scripts/dev-sandbox.sh up`），并显式给控制面地址：
//!
//! ```bash
//! POINTER_SANDBOX_URL=http://127.0.0.1:3000 \
//!   cargo test -p pointer-core --test remote_env_it -- --ignored --test-threads=1
//! ```
//!
//! 没设 `POINTER_SANDBOX_URL` 时测试直接返回（视为跳过），不会失败。

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use pointer_core::env::remote::{RemoteEnv, RemoteSpec};
use pointer_core::env::{Environment, ExecOptions, WalkOptions};

fn control_url() -> Option<String> {
    std::env::var("POINTER_SANDBOX_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn sh(script: &str) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), script.to_string()]
}

/// 沙箱在测试结束（含 panic）时回收。
struct Guard(RemoteEnv);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

#[test]
#[ignore = "需要 e2b-local 网关（dev-sandbox.sh 已配 volume_host_path）；手动跑"]
fn remote_env_mounts_host_directories_bidirectionally() {
    use pointer_core::env::mounts::{MountPlan, SkillRoot};

    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    // e2b-local 只接受 volume_host_path 下的**真实目录**（符号链接会被拒绝），
    // 所以沙箱根就用卷根：目录名即卷名。
    let volumes = std::env::var("E2B_LOCAL_VOLUMES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").expect("HOME"))
                .join(".e2b-local/volumes")
        });
    std::fs::create_dir_all(&volumes).expect("volume host path");
    // 上一轮残留的未注册目录会让 POST /volumes 报 "unmanaged"，先清掉
    for stale in ["it-user-home", "it-user-ws-it-conv", "it-skills"] {
        let _ = std::fs::remove_dir_all(volumes.join(stale));
    }
    // 技能卷要先注册（网关建目录），再往里写技能文件——自己预建目录会被判 unmanaged。
    pointer_core::env::remote::ensure_volume(&url, "it-skills").expect("register skills volume");
    let skills_host = volumes.join("it-skills");
    // 卷注册成功后目录由网关建；若上一轮被清理掉（注册记录还在），这里补建。
    std::fs::create_dir_all(&skills_host).expect("skills dir");
    std::fs::write(skills_host.join("SKILL.md"), "# demo\n").expect("skill file");

    let roots = vec![SkillRoot::new("skills", &skills_host)];
    let mounts = MountPlan::for_session(&volumes, "it-user", "it-conv", &roots);
    let host_home = mounts[0].host.clone();
    let host_workspace = mounts[1].host.clone();

    let spec = RemoteSpec {
        control_url: url,
        mounts,
        volume_host_path: Some(volumes.clone()),
        ..Default::default()
    };
    let env = RemoteEnv::create(&spec).expect("create sandbox with volumes");
    eprintln!("sandbox={} envd={}", env.sandbox_id(), env.envd_url());
    let env = Guard(env);
    let env = &env.0;

    // 宿主 → 沙箱
    std::fs::write(host_workspace.join("from-host.txt"), "hello-from-host\n").expect("host write");
    assert_eq!(
        env.read_to_string(Path::new("/workspace/from-host.txt"))
            .expect("read host file in sandbox"),
        "hello-from-host\n"
    );
    std::fs::write(host_home.join("home-file.txt"), "from-host-home").expect("host home write");
    assert_eq!(
        env.read_to_string(Path::new("/home/pointer/home-file.txt"))
            .expect("read home file"),
        "from-host-home"
    );

    // 沙箱 → 宿主
    env.write_file(Path::new("/workspace/from-sandbox.txt"), b"hello-from-sandbox")
        .expect("sandbox write");
    let landed = host_workspace.join("from-sandbox.txt");
    assert!(landed.exists(), "宿主目录应看到沙箱写入: {}", landed.display());
    assert_eq!(std::fs::read(&landed).unwrap(), b"hello-from-sandbox");

    // 技能根也挂上了（e2b-local 上只能是 rw）
    assert_eq!(
        env.read_to_string(Path::new("/skills/skills/SKILL.md"))
            .expect("read skill"),
        "# demo\n"
    );
}

#[test]
#[ignore = "需要 e2b-local 网关；设 POINTER_SANDBOX_URL 后手动跑"]
fn remote_env_roundtrip_against_e2b_local() {
    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let spec = RemoteSpec {
        control_url: url,
        timeout_secs: 600,
        ..Default::default()
    };
    let env = RemoteEnv::create(&spec).expect("create sandbox");
    eprintln!("sandbox={} envd={}", env.sandbox_id(), env.envd_url());
    let env = Guard(env);
    let env = &env.0;

    // --- 文件 IO -------------------------------------------------------
    let a = Path::new("/workspace/a.txt");
    env.write_file(a, b"hello").expect("write a.txt");
    assert_eq!(env.read_file(a).expect("read a.txt"), b"hello");
    assert_eq!(env.read_to_string(a).expect("read_to_string"), "hello");

    let meta = env.metadata(a).expect("metadata a.txt");
    assert!(meta.is_file && !meta.is_dir && !meta.is_symlink, "{meta:?}");
    assert_eq!(meta.len, 5);
    assert!(env.exists(a));
    assert!(!env.exists(Path::new("/workspace/nope.txt")));

    env.create_dir_all(Path::new("/workspace/sub/deep")).expect("mkdir -p");
    env.write_file(Path::new("/workspace/sub/deep/b.txt"), b"deep")
        .expect("write b.txt");

    let listed = env.list_dir(Path::new("/workspace")).expect("list /workspace");
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"a.txt"), "{names:?}");
    assert!(names.contains(&"sub"), "{names:?}");
    assert!(listed.iter().any(|e| e.name == "sub" && e.is_dir));

    let walked = env
        .walk(Path::new("/workspace"), WalkOptions::new(3))
        .expect("walk");
    assert!(walked.len() >= 3, "{walked:?}");
    assert!(walked.iter().any(|e| e.path.ends_with("sub/deep/b.txt")));

    // --- exec ----------------------------------------------------------
    let out = env
        .exec(&sh("echo hi; echo err >&2; exit 3"), ExecOptions::new())
        .expect("exec exit 3");
    assert_eq!(out.status, 3, "{out:?}");
    assert_eq!(out.stdout_str(), "hi\n");
    assert_eq!(out.stderr_str(), "err\n");
    assert!(!out.timed_out);

    let out = env
        .exec(
            &sh("pwd; echo FOO=$FOO"),
            ExecOptions::new()
                .with_cwd("/workspace")
                .with_env("FOO", "bar"),
        )
        .expect("exec cwd/env");
    assert_eq!(out.stdout_str(), "/workspace\nFOO=bar\n");

    // env_remove：先设再删
    let out = env
        .exec(
            &sh("echo FOO=[$FOO]"),
            ExecOptions::new()
                .with_env("FOO", "should-be-removed")
                .without_env("FOO"),
        )
        .expect("exec env_remove");
    assert_eq!(out.stdout_str(), "FOO=[]\n");

    // stdin 重定向
    let out = env
        .exec(
            &sh("cat"),
            ExecOptions::new().with_stdin(b"piped-data".to_vec()),
        )
        .expect("exec stdin");
    assert_eq!(out.stdout, b"piped-data");

    // 输出上限
    let out = env
        .exec(
            &sh("printf '0123456789'"),
            ExecOptions::new().with_max_output_bytes(4),
        )
        .expect("exec cap");
    assert_eq!(out.stdout, b"0123");
    assert!(out.stdout_truncated);

    // 超时
    let started = std::time::Instant::now();
    let out = env
        .exec(
            &sh("sleep 5"),
            ExecOptions::new().with_timeout(Duration::from_secs(1)),
        )
        .expect("exec timeout");
    let elapsed = started.elapsed();
    assert!(out.timed_out || out.status != 0, "超时没生效: {out:?} ({elapsed:?})");

    // --- 其余 Environment 操作 -----------------------------------------
    let real = env.canonicalize(Path::new("/workspace/../workspace/a.txt")).expect("realpath");
    assert_eq!(real, Path::new("/workspace/a.txt"));

    env.set_executable(a).expect("chmod");
    let out = env.exec(&sh("test -x /workspace/a.txt"), ExecOptions::new()).expect("test -x");
    assert!(out.success(), "set_executable 没生效: {out:?}");

    env.remove_file(a).expect("rm file");
    assert!(!env.exists(a));
    env.remove_dir_all(Path::new("/workspace/sub")).expect("rm -rf");
    assert!(!env.exists(Path::new("/workspace/sub")));

    // --- 流式读（Range 分块，D）----------------------------------------
    let big = vec![b'x'; 9 * 1024 * 1024];
    env.write_file(Path::new("/workspace/big.bin"), &big)
        .expect("write big.bin");
    let mut reader = env
        .open_reader(Path::new("/workspace/big.bin"))
        .expect("open_reader");
    let mut total = 0usize;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = reader.read(&mut buf).expect("read chunk");
        if n == 0 {
            break;
        }
        assert!(buf[..n].iter().all(|b| *b == b'x'), "内容被破坏");
        total += n;
    }
    assert_eq!(total, big.len(), "流式读字节数应与文件一致");
    env.remove_file(Path::new("/workspace/big.bin")).expect("rm big");

    // 只读路径：procfs 即使 root 也不可写（rootfs 本身是可写 overlay，不能拿 /etc 当判据）
    assert!(
        env.write_file(Path::new("/proc/pointer-should-fail"), b"x")
            .is_err(),
        "procfs 写入应当失败"
    );
}

#[test]
#[ignore = "需要 e2b-local + pointer-sandbox-headless 镜像；手动跑"]
fn media_pipeline_runs_inside_sandbox() {
    use pointer_core::env::{Environment, EnvironmentGuard};
    use pointer_core::media::exec::MediaInput;
    use pointer_core::media::video::{
        extract_video_frames_with_range, probe_video_duration_at, VideoTimeRange,
    };
    use std::sync::Arc;

    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let template = std::env::var("POINTER_SANDBOX_TEMPLATE")
        .unwrap_or_else(|_| "pointer-sandbox-headless:latest".to_string());

    // 1) 先在宿主生成一段 2 秒测试视频（此时还没有绑定环境 → 跑在宿主）
    let host_dir = tempfile::tempdir().expect("host tempdir");
    let host_video = host_dir.path().join("in.mp4");
    let gen_args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=64x64:d=2",
        "-c:v",
        "libx264",
        "-y",
        host_video.to_str().unwrap(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let gen = pointer_core::media::exec::run("ffmpeg", &gen_args).expect("host ffmpeg");
    assert!(gen.success(), "宿主生成测试视频失败: {}", gen.stderr_str());
    let video_bytes = std::fs::read(&host_video).expect("read host video");

    // 2) 起沙箱（媒体镜像）
    let spec = RemoteSpec {
        control_url: url,
        template,
        ..Default::default()
    };
    let remote = Arc::new(RemoteEnv::create(&spec).expect("create sandbox"));
    let _cleanup = ArcGuard(remote.clone());
    let env: Arc<dyn Environment> = remote.clone();

    // 3) 把视频写进沙箱，然后在沙箱里跑媒体管线
    env.write_file(Path::new("/workspace/in.mp4"), &video_bytes)
        .expect("upload video");
    let _bound = EnvironmentGuard::enter(env.clone());

    // 文件工具的路径判定必须走环境：宿主上根本没有 `/workspace`，
    // 用 Path::is_dir()/canonicalize() 会误判成"工作区不存在"。
    let ws = pointer_core::tools::file::resolve_tool_workspace_root()
        .expect("headless 下工作区解析应成功");
    assert_eq!(ws, Path::new("/workspace"));
    let resolved =
        pointer_core::tools::file::resolve_within_workspace_root(Path::new("/workspace"), "sub/a.txt")
            .expect("headless 下相对路径解析应成功");
    assert_eq!(resolved, Path::new("/workspace/sub/a.txt"));

    let input = MediaInput::existing("/workspace/in.mp4");
    assert!(!input.is_staged(), "路径模式不应产生宿主临时文件");

    let duration = probe_video_duration_at(&input).expect("probe in sandbox");
    assert!(duration > 1.0, "duration={duration}");

    let range =
        VideoTimeRange::default_first_window(duration, 1.0).expect("range");
    let (frames, meta) =
        extract_video_frames_with_range(&input, "in.mp4", &range).expect("frames in sandbox");
    assert!(!frames.is_empty(), "沙箱内应至少抽到一帧");
    assert!(meta.frame_count >= 1, "frame_count={}", meta.frame_count);
    eprintln!(
        "sandbox media ok: duration={duration:.2}s frames={} ",
        meta.frame_count
    );
}

/// `Arc<RemoteEnv>` 的清理守卫：drop 时销毁沙箱，避免泄漏。
struct ArcGuard(std::sync::Arc<RemoteEnv>);

impl Drop for ArcGuard {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

#[test]
#[ignore = "需要本地沙箱网关（scripts/dev-sandbox.sh up）"]
fn remote_spawn_streams_stdin_and_stdout() {
    use pointer_core::env::{ChildHandle, SpawnOptions};
    use std::io::{BufRead, Write};

    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let template = std::env::var("POINTER_SANDBOX_TEMPLATE")
        .unwrap_or_else(|_| "pointer-sandbox-headless:latest".to_string());
    let spec = RemoteSpec {
        control_url: url,
        template,
        ..Default::default()
    };
    let env = RemoteEnv::create(&spec).expect("create sandbox");
    let cleanup = Guard2(&env);

    let run = || -> Result<(), String> {
        let argv = vec!["/bin/cat".to_string()];
        let mut child = env
            .spawn(&argv, SpawnOptions::default())
            .map_err(|e| format!("spawn: {e:#}"))?;
        child
            .write_all(b"hello\n")
            .map_err(|e| format!("write_all: {e:#}"))?;
        child
            .close_stdin()
            .map_err(|e| format!("close_stdin: {e:#}"))?;

        let mut reader = child.reader().map_err(|e| format!("reader: {e:#}"))?;
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("read_line: {e:#}"))?;
        if line != "hello\n" {
            return Err(format!("stdout 不匹配: {line:?}"));
        }
        let code = child.wait().map_err(|e| format!("wait: {e:#}"))?;
        if code != 0 {
            return Err(format!("退出码 {code}"));
        }
        let _ = child.kill();
        Ok(())
    };

    // 不再做 skip 兜底：e2b-local 实测已可用，回归就该红。
    run().unwrap_or_else(|e| panic!("remote spawn 失败: {e}"));
    eprintln!("remote spawn ok: stdin/stdout 双向流可用");
    let _ = &cleanup;
}

/// 不销毁沙箱的守卫（测试内部还要用它，drop 时再删）。
struct Guard2<'a>(&'a RemoteEnv);

impl Drop for Guard2<'_> {
    fn drop(&mut self) {
        let _ = self.0.destroy();
    }
}

#[test]
#[ignore = "需要本地沙箱网关（scripts/dev-sandbox.sh up）"]
fn plugin_hook_script_runs_from_mounted_plugins_dir() {
    use pointer_core::env::mounts::MountPlan;
    use pointer_core::env::{ChildHandle, SpawnOptions};
    use std::io::{BufRead, Write};

    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let volumes = std::env::var("E2B_LOCAL_VOLUMES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").expect("HOME")).join(".e2b-local/volumes")
        });
    std::fs::create_dir_all(&volumes).expect("volume host path");
    for stale in ["ph-user-home", "ph-user-ws-ph-conv", "ph-plugins"] {
        let _ = std::fs::remove_dir_all(volumes.join(stale));
    }

    // 插件卷：先注册（网关建目录），再写 hook 脚本。
    pointer_core::env::remote::ensure_volume(&url, "ph-plugins").expect("register plugins volume");
    let plugins_host = volumes.join("ph-plugins");
    std::fs::create_dir_all(plugins_host.join("demo/hooks")).expect("plugins dir");
    let script_host = plugins_host.join("demo/hooks/echo.sh");
    std::fs::write(
        &script_host,
        "#!/bin/sh\nread line\nprintf '{\"decision\":\"block\",\"reason\":\"%s\"}' \"$line\"\n",
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_host, std::fs::Permissions::from_mode(0o755))
            .expect("chmod hook");
    }

    let mounts = MountPlan::for_session_with_plugins(
        &volumes,
        "ph-user",
        "ph-conv",
        &[],
        Some(&plugins_host),
        None,
    );
    assert!(
        mounts.iter().any(|m| m.read_only && m.guest == std::path::Path::new("/plugins")),
        "插件根应只读挂到 /plugins：{mounts:?}"
    );

    let spec = RemoteSpec {
        control_url: url,
        mounts,
        volume_host_path: Some(volumes.clone()),
        template: std::env::var("POINTER_SANDBOX_TEMPLATE")
            .unwrap_or_else(|_| "pointer-sandbox-headless:latest".to_string()),
        ..Default::default()
    };
    let env = RemoteEnv::create(&spec).expect("create sandbox with plugins");
    let cleanup = Guard2(&env);

    // 宿主插件路径 → 沙箱路径
    let guest = env
        .path_map()
        .to_agent(&script_host)
        .expect("map plugin path");
    assert_eq!(guest, std::path::Path::new("/plugins/demo/hooks/echo.sh"));

    // 真的在沙箱里执行一次 hook：写事件 → 关 stdin → 读决策
    let argv = vec!["/bin/sh".to_string(), "/plugins/demo/hooks/echo.sh".to_string()];
    let mut child = env.spawn(&argv, SpawnOptions::default()).expect("spawn hook");
    child
        .write_all(br#"{"hook_event_name":"PreToolUse","tool_name":"shell"}"#)
        .expect("write event");
    child.close_stdin().expect("close stdin");
    let mut reader = child.reader().expect("reader");
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read decision");
    let code = child.wait().expect("wait");
    assert_eq!(code, 0, "hook 退出码");
    assert!(out.contains("\"block\""), "决策应为 block：{out}");
    assert!(out.contains("PreToolUse"), "应收到事件内容：{out}");

    let _ = &cleanup;
}

#[test]
#[ignore = "需要本地沙箱网关（scripts/dev-sandbox.sh up）"]
fn remote_env_is_safe_inside_async_context() {
    // 回归：reqwest::blocking 在 tokio 线程上跑会 panic
    // 「Cannot drop a runtime in a context where blocking is not allowed」。
    // 真实链路里 bind_and_resolve_workspace / 工具调用都在 async 上下文，
    // 所以这里在 runtime 内建沙箱 + 读写 + exec，必须不 panic。
    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let template = std::env::var("POINTER_SANDBOX_TEMPLATE")
        .unwrap_or_else(|_| "pointer-sandbox-headless:latest".to_string());
    let spec = RemoteSpec {
        control_url: url,
        template,
        ..Default::default()
    };
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        let env = RemoteEnv::create(&spec).expect("create inside async");
        let _guard = Guard2(&env);
        env.write_file(Path::new("/tmp/async.txt"), b"async-ok\n")
            .expect("write inside async");
        assert_eq!(
            env.read_to_string(Path::new("/tmp/async.txt")).expect("read"),
            "async-ok\n"
        );
        let out = env
            .exec(
                &["/bin/echo".to_string(), "hi".to_string()],
                ExecOptions::default(),
            )
            .expect("exec inside async");
        assert_eq!(out.status, 0);
        assert_eq!(out.stdout_str().trim(), "hi");
    });
}

#[test]
#[ignore = "需要本地沙箱网关（scripts/dev-sandbox.sh up）"]
fn terminal_tool_runs_inside_sandbox() {
    // 回归：terminal 曾经直连宿主 std::process::Command，headless 下仍跑在本机。
    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let template = std::env::var("POINTER_SANDBOX_TEMPLATE")
        .unwrap_or_else(|_| "pointer-sandbox-headless:latest".to_string());
    let spec = RemoteSpec {
        control_url: url,
        template,
        ..Default::default()
    };
    let env = std::sync::Arc::new(RemoteEnv::create(&spec).expect("create sandbox"));
    let cleanup = Guard2(&env);
    let _guard = pointer_core::env::EnvironmentGuard::enter(env.clone());

    // 工具分派用这个函数拿工作区：headless 下必须给沙箱路径，不能回落到宿主 settings。
    assert_eq!(
        pointer_core::workspace_resolve::resolve_tool_workspace(
            "c1",
            None,
            "/Users/someone/host-workspace",
            "test",
        ),
        "/workspace"
    );

    let args = serde_json::json!({
        "command": "uname -s; echo HOME=$HOME; pwd"
    });
    let result = pointer_core::tools::terminal::run_terminal_command_streaming(
        args,
        "/workspace".to_string(),
        |_| {},
        None,
        None,
        None,
        None,
    )
    .expect("terminal tool");

    assert_eq!(result.exit_code, Some(0), "stderr={}", result.stderr);
    assert!(
        result.stdout.contains("Linux"),
        "应跑在沙箱（Linux）而不是宿主：{}",
        result.stdout
    );
    assert!(
        result.stdout.contains("/workspace"),
        "cwd 应为沙箱工作区：{}",
        result.stdout
    );

    let _ = &cleanup;
}

#[test]
#[ignore = "需要本地沙箱网关（scripts/dev-sandbox.sh up）"]
fn remote_pty_gives_a_tty_and_supports_resize() {
    use pointer_core::env::PtySpec;

    let Some(url) = control_url() else {
        eprintln!("skip: POINTER_SANDBOX_URL 未设置");
        return;
    };
    let spec = RemoteSpec {
        control_url: url,
        template: std::env::var("POINTER_SANDBOX_TEMPLATE")
            .unwrap_or_else(|_| "pointer-sandbox-headless:latest".to_string()),
        ..Default::default()
    };
    let env = RemoteEnv::create(&spec).expect("create sandbox");
    let guard = Guard(env);
    let env = &guard.0;

    let mut pty_spec = PtySpec::new(sh("tty; stty size; sleep 0.8; stty size").to_vec());
    pty_spec.cols = 100;
    pty_spec.rows = 40;
    let pty = env.pty_open(pty_spec).expect("open remote pty");
    let mut reader = pty.take_reader().expect("take reader");
    std::thread::sleep(Duration::from_millis(300));
    pty.resize(120, 50).expect("resize");

    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read pty");
    assert!(out.contains("/dev/"), "沙箱里应分配到真实 tty：{out}");
    assert!(out.contains("40 100"), "初始尺寸应为 40 100：{out}");
    assert!(out.contains("50 120"), "resize 后应为 50 120：{out}");
    let _ = pty.kill();
}
