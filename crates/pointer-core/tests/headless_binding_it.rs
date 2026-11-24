//! headless 档位的会话绑定真机测试（需要 e2b-local 网关）。
//!
//! ```bash
//! cargo test -p pointer-core --test headless_binding_it -- --ignored --test-threads=1
//! ```
//!
//! 覆盖 §3.4 / §7.1 B3 的三条主线：
//! 1. 部署默认 headless → 会话绑到远端，工作区根变成 guest `/data/workspaces`；
//! 2. admin 账号默认 local → 不建沙箱、工作区根保持宿主路径；
//! 3. 会话回收 → 沙箱销毁、缓存清空。

use std::path::PathBuf;

use pointer_core::env::{current_env, effective_env, session_env, EnvKind, Environment};

fn sandbox_url() -> String {
    std::env::var("POINTER_SANDBOX_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:3000".to_string())
}

#[test]
#[ignore = "需要 e2b-local 网关；手动跑"]
fn headless_binding_end_to_end() {
    // 部署默认 headless；admin 名单里只有 admin。
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_PROVIDER", "headless");
    std::env::set_var("POINTER_SERVER_ENVIRONMENT_SANDBOX_URL", sandbox_url());
    std::env::set_var("POINTER_SERVER_ADMIN_ACCOUNTS", "admin");
    std::env::remove_var("POINTER_SERVER_ENVIRONMENT_ACCOUNTS");

    // 技能根：默认指向干净的临时目录；外部已给 POINTER_SKILLS_DIR 时尊重它
    // （用来对真实技能库做验证）。真实库里若出现 token.txt，会被挂载前的凭证检查拦下
    // ——那是设计行为，不是测试缺陷。
    let _skills_guard: Option<tempfile::TempDir>;
    if std::env::var("POINTER_SKILLS_DIR")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        let skills = tempfile::tempdir().expect("temp skills dir");
        std::fs::create_dir_all(skills.path().join("demo")).unwrap();
        std::fs::write(skills.path().join("demo/SKILL.md"), "# demo\n").unwrap();
        std::env::set_var("POINTER_SKILLS_DIR", skills.path());
        _skills_guard = Some(skills);
    } else {
        _skills_guard = None;
    }

    // --- 1. 普通用户：绑到远端 ---------------------------------------
    let conv = "conv-headless-binding-it";
    let (guard, workspace) = session_env::bind_and_resolve_workspace(
        conv,
        "alice",
        PathBuf::from("/host/should-be-ignored"),
    )
    .expect("bind headless session");
    assert!(guard.is_some(), "headless 档位必须绑定环境");
    assert_eq!(workspace, "/data/workspaces", "headless 下工作区根是 guest 路径");
    assert_eq!(effective_env().kind(), EnvKind::Headless);
    assert!(current_env().is_some());

    let sandbox_id = session_env::cached_sandbox_id(conv).expect("沙箱应被缓存");
    eprintln!("sandbox={sandbox_id}");

    // 文件写入真的落在沙箱里
    let env = effective_env();
    let path = std::path::Path::new("/data/workspaces/binding.txt");
    env.write_file(path, b"from-headless").expect("write in sandbox");
    assert_eq!(env.read_file(path).expect("read back"), b"from-headless");

    // 同一会话再绑一次：复用同一个沙箱
    let (guard2, workspace2) =
        session_env::bind_and_resolve_workspace(conv, "alice", PathBuf::from("/host/x"))
            .expect("rebind");
    // 已经绑过 → 不新建、不报错
    assert!(guard2.is_none(), "已有绑定不应重复进入");
    assert_eq!(workspace2, "/data/workspaces");
    assert_eq!(
        session_env::cached_sandbox_id(conv).as_deref(),
        Some(sandbox_id.as_str()),
        "同会话必须复用同一个沙箱"
    );
    drop(guard2);
    // 解绑 thread-local，才能验证 admin 分支（同一线程已绑定时会复用现有环境）
    drop(guard);

    // --- 2. admin 账号：默认 local，但**也要绑** LocalEnv（宿主直跑） ----
    let admin_conv = "conv-headless-binding-admin";
    let (admin_guard, admin_workspace) = session_env::bind_and_resolve_workspace(
        admin_conv,
        "admin",
        PathBuf::from("/host/admin-workspace"),
    )
    .expect("bind admin session");
    // local 档位不绑的话，工具线程会退到 DeniedEnv：宿主工具全线报
    // 「工作区目录无效或不存在」。绑了才能"admin 走宿主 + 普通用户走沙箱"并存。
    assert!(admin_guard.is_some(), "admin 是 local 档位，必须绑定 LocalEnv");
    assert_eq!(admin_workspace, "/host/admin-workspace", "local 档位工作区是宿主路径");
    assert_eq!(effective_env().kind(), EnvKind::Local);
    assert!(!effective_env().is_denied());
    assert!(session_env::cached_sandbox_id(admin_conv).is_none());
    drop(admin_guard);
    session_env::release(admin_conv);

    // --- 3. 回收 ------------------------------------------------------
    session_env::release(conv);
    assert!(
        session_env::cached_sandbox_id(conv).is_none(),
        "release 后缓存必须清空"
    );
    session_env::release_all();
}


/// 会话 → 沙箱的登记表：建了要记、释放要清，且释放后沙箱真的销毁。
///
/// docker 后端靠容器名（`pointer-sbx-<会话>`）认回，envd 的 sandboxID 是网关
/// 生成的，不记下来就只能靠超时回收（实测攒了 26 个 e2b-envd-* 容器）。
#[test]
#[ignore = "需要 e2b-local 网关；手动跑"]
fn headless_sandbox_registry_tracks_lifecycle() {
    use pointer_core::env::provider::{Backend, ProviderConfig, Tier};

    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var(
        "POINTER_SANDBOX_REGISTRY",
        dir.path().join("sandbox-registry.json"),
    );
    // 干净技能根：真实技能库里若有 token.txt，会被挂载前的凭证检查拦下。
    let skills = dir.path().join("skills");
    std::fs::create_dir_all(skills.join("demo")).unwrap();
    std::fs::write(skills.join("demo/SKILL.md"), "# demo\n").unwrap();
    std::env::set_var("POINTER_SKILLS_DIR", &skills);

    let conv = "conv-headless-registry-it";
    let cfg = ProviderConfig {
        admin_tier: Default::default(),
        tier: Some(Tier::Headless),
        backend: Some(Backend::Envd),
        sandbox_url: Some(sandbox_url()),
        volume_host_path: None,
        template: Some("pointer-sandbox-headless:latest".to_string()),
        admins: Vec::new(),
        accounts: Default::default(),
        tier_backends: Default::default(),
        platform: true,
    };

    let env = pointer_core::env::session_env::resolve_session_env(
        &cfg,
        conv,
        "registry-user",
        Some(dir.path()),
    )
        .expect("resolve")
        .expect("envd 后端应返回环境");

    let entry = pointer_core::env::sandbox_registry::lookup(conv).expect("建完应登记");
    assert!(
        entry.sandbox_id.starts_with("sbx"),
        "登记的是网关的 sandboxID：{}",
        entry.sandbox_id
    );
    assert_eq!(entry.template, "pointer-sandbox-headless:latest");

    // 环境确实是那个沙箱（能执行命令）。
    let out = env
        .exec(&["echo".to_string(), "ok".to_string()], Default::default())
        .expect("exec");
    assert!(out.success());

    session_env::release(conv);
    assert!(
        pointer_core::env::sandbox_registry::lookup(conv).is_none(),
        "释放后应清掉登记"
    );

    // 释放 = 销毁：拿旧 id 再 attach 探活，应该已经死了。
    let spec = pointer_core::env::remote::RemoteSpec {
        control_url: sandbox_url(),
        template: "pointer-sandbox-headless:latest".to_string(),
        ..Default::default()
    };
    let alive = pointer_core::env::remote::RemoteEnv::attach(&spec, entry.sandbox_id, entry.envd_url)
        .map(|probe| probe.probe_alive())
        .unwrap_or(false);
    assert!(!alive, "释放后沙箱应已销毁");
}
